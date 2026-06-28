use std::collections::BTreeMap;
use std::sync::Mutex;

use cachebox_client::ai::{
    EmbeddingCacheKeyInput, PromptCacheKeyInput, PromptMessage, embedding_cache_key,
    prompt_cache_key,
};
use cachebox_client::{
    ClientError, GetResult as RustGetResult, LeaseStartResult as RustLeaseStartResult, NativeClient,
};
use cachebox_protocol::{ContentType, ErrorCode, Metadata as RustMetadata, Ttl};
use pyo3::exceptions::{PyException, PyRuntimeError, PyValueError};
use pyo3::prelude::*;
use pyo3::types::{PyAny, PyDict, PyFloat, PyList, PyString};
use serde_json::{Number, Value};
use tokio::runtime::Runtime;

pyo3::create_exception!(_cachebox, CacheboxError, PyException);
pyo3::create_exception!(_cachebox, ServerError, CacheboxError);

#[pyclass]
#[derive(Clone)]
struct Metadata {
    #[pyo3(get, set)]
    ttl_ms: Option<u64>,
    #[pyo3(get, set)]
    stale_ttl_ms: Option<u64>,
    #[pyo3(get, set)]
    cost: Option<u64>,
    #[pyo3(get, set)]
    tags: Vec<String>,
    #[pyo3(get, set)]
    content_type: String,
}

#[pymethods]
impl Metadata {
    #[new]
    #[pyo3(signature = (ttl_ms=None, stale_ttl_ms=None, cost=None, tags=None, content_type="application/octet-stream".to_string()))]
    fn new(
        ttl_ms: Option<u64>,
        stale_ttl_ms: Option<u64>,
        cost: Option<u64>,
        tags: Option<Vec<String>>,
        content_type: String,
    ) -> Self {
        Self {
            ttl_ms,
            stale_ttl_ms,
            cost,
            tags: tags.unwrap_or_default(),
            content_type,
        }
    }

    fn __repr__(&self) -> String {
        format!(
            "Metadata(ttl_ms={:?}, stale_ttl_ms={:?}, cost={:?}, tags={:?}, content_type={:?})",
            self.ttl_ms, self.stale_ttl_ms, self.cost, self.tags, self.content_type
        )
    }
}

impl TryFrom<Metadata> for RustMetadata {
    type Error = PyErr;

    fn try_from(metadata: Metadata) -> Result<Self, Self::Error> {
        let content_type = match metadata.content_type.as_str() {
            "application/octet-stream" | "octet-stream" => ContentType::OctetStream,
            "other" => ContentType::Other,
            _ => {
                return Err(PyValueError::new_err(
                    "content_type must be 'application/octet-stream', 'octet-stream', or 'other'",
                ));
            }
        };

        Ok(Self {
            ttl: metadata.ttl_ms.map(|milliseconds| Ttl { milliseconds }),
            stale_ttl: metadata
                .stale_ttl_ms
                .map(|milliseconds| Ttl { milliseconds }),
            cost: metadata.cost,
            tags: metadata.tags,
            content_type,
        })
    }
}

#[pyclass]
#[derive(Clone, PartialEq, Eq)]
struct GetResult {
    #[pyo3(get)]
    state: String,
    #[pyo3(get)]
    value: Option<Vec<u8>>,
}

#[pymethods]
impl GetResult {
    #[staticmethod]
    fn hit(value: Vec<u8>) -> Self {
        Self {
            state: "hit".to_string(),
            value: Some(value),
        }
    }

    #[staticmethod]
    fn stale(value: Vec<u8>) -> Self {
        Self {
            state: "stale".to_string(),
            value: Some(value),
        }
    }

    #[staticmethod]
    fn miss() -> Self {
        Self {
            state: "miss".to_string(),
            value: None,
        }
    }

    fn __repr__(&self) -> String {
        match &self.value {
            Some(value) => format!("GetResult.{}({value:?})", self.state),
            None => format!("GetResult.{}()", self.state),
        }
    }

    fn __richcmp__(&self, other: PyRef<'_, Self>, op: pyo3::basic::CompareOp) -> bool {
        match op {
            pyo3::basic::CompareOp::Eq => self == &*other,
            pyo3::basic::CompareOp::Ne => self != &*other,
            _ => false,
        }
    }
}

impl From<RustGetResult> for GetResult {
    fn from(result: RustGetResult) -> Self {
        match result {
            RustGetResult::Hit(value) => Self::hit(value),
            RustGetResult::Stale(value) => Self::stale(value),
            RustGetResult::Miss => Self::miss(),
        }
    }
}

#[pyclass]
#[derive(Clone, PartialEq, Eq)]
struct LeaseStartResult {
    #[pyo3(get)]
    state: String,
    #[pyo3(get)]
    value: Option<Vec<u8>>,
    #[pyo3(get)]
    lease_token: Option<String>,
    #[pyo3(get)]
    stale_value: Option<Vec<u8>>,
}

#[pymethods]
impl LeaseStartResult {
    fn __repr__(&self) -> String {
        format!(
            "LeaseStartResult(state={:?}, value={:?}, lease_token={:?}, stale_value={:?})",
            self.state, self.value, self.lease_token, self.stale_value
        )
    }

    fn __richcmp__(&self, other: PyRef<'_, Self>, op: pyo3::basic::CompareOp) -> bool {
        match op {
            pyo3::basic::CompareOp::Eq => self == &*other,
            pyo3::basic::CompareOp::Ne => self != &*other,
            _ => false,
        }
    }
}

impl From<RustLeaseStartResult> for LeaseStartResult {
    fn from(result: RustLeaseStartResult) -> Self {
        match result {
            RustLeaseStartResult::Hit(value) => Self {
                state: "hit".to_string(),
                value: Some(value),
                lease_token: None,
                stale_value: None,
            },
            RustLeaseStartResult::Stale(value) => Self {
                state: "stale".to_string(),
                value: Some(value),
                lease_token: None,
                stale_value: None,
            },
            RustLeaseStartResult::LeaseGranted {
                lease_token,
                stale_value,
            } => Self {
                state: "lease_granted".to_string(),
                value: None,
                lease_token: Some(lease_token),
                stale_value,
            },
            RustLeaseStartResult::LeaseDenied => Self {
                state: "lease_denied".to_string(),
                value: None,
                lease_token: None,
                stale_value: None,
            },
        }
    }
}

#[pyclass]
struct Client {
    runtime: Runtime,
    inner: Mutex<NativeClient>,
}

#[pymethods]
impl Client {
    #[staticmethod]
    fn connect_tcp(addr: String) -> PyResult<Self> {
        let runtime = Runtime::new().map_err(|error| CacheboxError::new_err(error.to_string()))?;
        let client = runtime
            .block_on(NativeClient::connect_tcp(addr))
            .map_err(client_error_to_py)?;
        Ok(Self {
            runtime,
            inner: Mutex::new(client),
        })
    }

    fn get(&self, namespace: String, key: Vec<u8>) -> PyResult<GetResult> {
        let mut client = self.lock_client()?;
        self.runtime
            .block_on(client.get(namespace, key))
            .map(GetResult::from)
            .map_err(client_error_to_py)
    }

    #[pyo3(signature = (namespace, key, value, metadata=None))]
    fn put(
        &self,
        namespace: String,
        key: Vec<u8>,
        value: Vec<u8>,
        metadata: Option<Metadata>,
    ) -> PyResult<u32> {
        let metadata = metadata.unwrap_or_else(|| {
            Metadata::new(
                None,
                None,
                None,
                None,
                "application/octet-stream".to_string(),
            )
        });
        let metadata = RustMetadata::try_from(metadata)?;
        let mut client = self.lock_client()?;
        self.runtime
            .block_on(client.put(namespace, key, metadata, value))
            .map_err(client_error_to_py)
    }

    fn delete(&self, namespace: String, key: Vec<u8>) -> PyResult<bool> {
        let mut client = self.lock_client()?;
        self.runtime
            .block_on(client.delete(namespace, key))
            .map_err(client_error_to_py)
    }

    fn batch_get(&self, namespace: String, keys: Vec<Vec<u8>>) -> PyResult<Vec<GetResult>> {
        let mut client = self.lock_client()?;
        self.runtime
            .block_on(client.batch_get(namespace, keys))
            .map(|items| items.into_iter().map(GetResult::from).collect())
            .map_err(client_error_to_py)
    }

    fn invalidate_tag(&self, namespace: String, tag: String) -> PyResult<u32> {
        let mut client = self.lock_client()?;
        self.runtime
            .block_on(client.invalidate_tag(namespace, tag))
            .map_err(client_error_to_py)
    }

    #[pyo3(signature = (namespace, key, lease_ttl_ms, allow_stale_ms=None))]
    fn start_lease(
        &self,
        namespace: String,
        key: Vec<u8>,
        lease_ttl_ms: u64,
        allow_stale_ms: Option<u64>,
    ) -> PyResult<LeaseStartResult> {
        let mut client = self.lock_client()?;
        self.runtime
            .block_on(client.start_lease(namespace, key, lease_ttl_ms, allow_stale_ms))
            .map(LeaseStartResult::from)
            .map_err(client_error_to_py)
    }

    #[pyo3(signature = (namespace, key, lease_token, value, metadata=None))]
    fn complete_lease(
        &self,
        namespace: String,
        key: Vec<u8>,
        lease_token: String,
        value: Vec<u8>,
        metadata: Option<Metadata>,
    ) -> PyResult<u32> {
        let metadata = metadata.unwrap_or_else(|| {
            Metadata::new(
                None,
                None,
                None,
                None,
                "application/octet-stream".to_string(),
            )
        });
        let metadata = RustMetadata::try_from(metadata)?;
        let mut client = self.lock_client()?;
        self.runtime
            .block_on(client.complete_lease(namespace, key, lease_token, metadata, value))
            .map_err(client_error_to_py)
    }
}

impl Client {
    fn lock_client(&self) -> PyResult<std::sync::MutexGuard<'_, NativeClient>> {
        self.inner
            .lock()
            .map_err(|_| PyRuntimeError::new_err("client lock poisoned"))
    }
}

fn client_error_to_py(error: ClientError) -> PyErr {
    if let ClientError::Server { code, message } = error {
        return ServerError::new_err(format!("{}: {message}", error_code_name(code)));
    }
    CacheboxError::new_err(error.to_string())
}

fn error_code_name(code: ErrorCode) -> &'static str {
    match code {
        ErrorCode::BadFrame => "BadFrame",
        ErrorCode::UnsupportedVersion => "UnsupportedVersion",
        ErrorCode::UnknownCommand => "UnknownCommand",
        ErrorCode::InvalidNamespace => "InvalidNamespace",
        ErrorCode::InvalidTag => "InvalidTag",
        ErrorCode::InvalidTtl => "InvalidTtl",
        ErrorCode::ValueTooLarge => "ValueTooLarge",
        ErrorCode::EntryTooLarge => "EntryTooLarge",
        ErrorCode::InsufficientMemory => "InsufficientMemory",
        ErrorCode::InvalidLeaseToken => "InvalidLeaseToken",
        ErrorCode::FrameTooLarge => "FrameTooLarge",
    }
}

#[pyfunction]
#[allow(clippy::too_many_arguments)]
#[pyo3(signature = (
    provider,
    model,
    application_namespace,
    messages,
    model_version=None,
    system_prompt=None,
    tool_schema=None,
    sampling_parameters=None,
    output_format=None,
    retrieval_context_hash=None
))]
fn ai_prompt_cache_key(
    provider: String,
    model: String,
    application_namespace: String,
    messages: &Bound<'_, PyAny>,
    model_version: Option<String>,
    system_prompt: Option<String>,
    tool_schema: Option<&Bound<'_, PyAny>>,
    sampling_parameters: Option<&Bound<'_, PyAny>>,
    output_format: Option<String>,
    retrieval_context_hash: Option<String>,
) -> PyResult<Vec<u8>> {
    let mut input = PromptCacheKeyInput::new(provider, model, application_namespace);
    input.model_version = model_version;
    input.system_prompt = system_prompt;
    input.messages = prompt_messages_from_py(messages)?;
    input.tool_schema = tool_schema.map(json_from_py).transpose()?;
    input.sampling_parameters = json_map_from_py(sampling_parameters)?;
    input.output_format = output_format;
    input.retrieval_context_hash = retrieval_context_hash;
    Ok(prompt_cache_key(&input))
}

#[pyfunction]
#[pyo3(signature = (
    model,
    input_content_hash,
    chunking_strategy,
    dimensions,
    application_namespace,
    model_version=None,
    normalization_settings=None
))]
fn ai_embedding_cache_key(
    model: String,
    input_content_hash: String,
    chunking_strategy: String,
    dimensions: u32,
    application_namespace: String,
    model_version: Option<String>,
    normalization_settings: Option<&Bound<'_, PyAny>>,
) -> PyResult<Vec<u8>> {
    let mut input = EmbeddingCacheKeyInput::new(
        model,
        input_content_hash,
        chunking_strategy,
        dimensions,
        application_namespace,
    );
    input.model_version = model_version;
    input.normalization_settings = json_map_from_py(normalization_settings)?;
    Ok(embedding_cache_key(&input))
}

fn prompt_messages_from_py(messages: &Bound<'_, PyAny>) -> PyResult<Vec<PromptMessage>> {
    let list = messages.downcast::<PyList>().map_err(|_| {
        PyValueError::new_err("messages must be a list of dictionaries with role/content fields")
    })?;
    list.iter()
        .map(|item| {
            let dict = item.downcast::<PyDict>().map_err(|_| {
                PyValueError::new_err(
                    "messages must be a list of dictionaries with role/content fields",
                )
            })?;
            let role = required_string(dict, "role")?;
            let content = required_string(dict, "content")?;
            let name = optional_string(dict, "name")?;
            Ok(PromptMessage {
                role,
                content,
                name,
            })
        })
        .collect()
}

fn required_string(dict: &Bound<'_, PyDict>, key: &str) -> PyResult<String> {
    dict.get_item(key)?
        .ok_or_else(|| PyValueError::new_err(format!("missing required field {key:?}")))?
        .extract()
}

fn optional_string(dict: &Bound<'_, PyDict>, key: &str) -> PyResult<Option<String>> {
    dict.get_item(key)?.map(|value| value.extract()).transpose()
}

fn json_map_from_py(value: Option<&Bound<'_, PyAny>>) -> PyResult<BTreeMap<String, Value>> {
    let Some(value) = value else {
        return Ok(BTreeMap::new());
    };
    let dict = value
        .downcast::<PyDict>()
        .map_err(|_| PyValueError::new_err("expected a dictionary"))?;
    let mut out = BTreeMap::new();
    for (key, value) in dict.iter() {
        out.insert(key.extract::<String>()?, json_from_py(&value)?);
    }
    Ok(out)
}

fn json_from_py(value: &Bound<'_, PyAny>) -> PyResult<Value> {
    if value.is_none() {
        return Ok(Value::Null);
    }
    if let Ok(value) = value.extract::<bool>() {
        return Ok(Value::Bool(value));
    }
    if let Ok(value) = value.extract::<i64>() {
        return Ok(Value::Number(Number::from(value)));
    }
    if let Ok(value) = value.extract::<u64>() {
        return Ok(Value::Number(Number::from(value)));
    }
    if value.downcast::<PyFloat>().is_ok() {
        let value = value.extract::<f64>()?;
        let number = Number::from_f64(value)
            .ok_or_else(|| PyValueError::new_err("JSON numbers must be finite"))?;
        return Ok(Value::Number(number));
    }
    if value.downcast::<PyString>().is_ok() {
        return Ok(Value::String(value.extract()?));
    }
    if let Ok(list) = value.downcast::<PyList>() {
        return list.iter().map(|item| json_from_py(&item)).collect();
    }
    if let Ok(dict) = value.downcast::<PyDict>() {
        let mut out = serde_json::Map::new();
        for (key, value) in dict.iter() {
            out.insert(key.extract::<String>()?, json_from_py(&value)?);
        }
        return Ok(Value::Object(out));
    }

    Err(PyValueError::new_err(
        "expected a JSON-compatible value: None, bool, int, float, str, list, or dict",
    ))
}

#[pymodule]
fn _cachebox(py: Python<'_>, module: &Bound<'_, PyModule>) -> PyResult<()> {
    module.add_class::<Client>()?;
    module.add_class::<GetResult>()?;
    module.add_class::<LeaseStartResult>()?;
    module.add_class::<Metadata>()?;
    module.add_function(wrap_pyfunction!(ai_prompt_cache_key, module)?)?;
    module.add_function(wrap_pyfunction!(ai_embedding_cache_key, module)?)?;
    module.add("CacheboxError", py.get_type::<CacheboxError>())?;
    module.add("ServerError", py.get_type::<ServerError>())?;
    Ok(())
}
