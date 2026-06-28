use std::sync::Mutex;

use cachebox_client::{
    ClientError, GetResult as RustGetResult, LeaseStartResult as RustLeaseStartResult, NativeClient,
};
use cachebox_protocol::{ContentType, ErrorCode, Metadata as RustMetadata, Ttl};
use pyo3::exceptions::{PyException, PyRuntimeError, PyValueError};
use pyo3::prelude::*;
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

#[pymodule]
fn _cachebox(py: Python<'_>, module: &Bound<'_, PyModule>) -> PyResult<()> {
    module.add_class::<Client>()?;
    module.add_class::<GetResult>()?;
    module.add_class::<LeaseStartResult>()?;
    module.add_class::<Metadata>()?;
    module.add("CacheboxError", py.get_type::<CacheboxError>())?;
    module.add("ServerError", py.get_type::<ServerError>())?;
    Ok(())
}
