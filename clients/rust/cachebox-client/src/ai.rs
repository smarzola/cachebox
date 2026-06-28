//! AI-oriented client helpers.
//!
//! These helpers are intentionally provider-neutral. They build deterministic
//! cache keys from structured request metadata, but they do not interpret prompt
//! semantics or call model providers.

use std::collections::BTreeMap;

use serde_json::Value;

const PROMPT_KEY_PREFIX: &[u8] = b"ai:prompt:v1:";
const EMBEDDING_KEY_PREFIX: &[u8] = b"ai:embedding:v1:";
const PROMPT_NORMALIZATION_VERSION: &[u8] = b"cachebox.ai.prompt.v1";
const EMBEDDING_NORMALIZATION_VERSION: &[u8] = b"cachebox.ai.embedding.v1";
const FNV_OFFSET_BASIS: u128 = 0x6c62272e07bb014262b821756295c58d;
const FNV_PRIME: u128 = 0x0000000001000000000000000000013b;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PromptMessage {
    pub role: String,
    pub content: String,
    pub name: Option<String>,
}

impl PromptMessage {
    pub fn new(role: impl Into<String>, content: impl Into<String>) -> Self {
        Self {
            role: role.into(),
            content: content.into(),
            name: None,
        }
    }

    pub fn with_name(mut self, name: impl Into<String>) -> Self {
        self.name = Some(name.into());
        self
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct PromptCacheKeyInput {
    pub provider: String,
    pub model: String,
    pub model_version: Option<String>,
    pub messages: Vec<PromptMessage>,
    pub system_prompt: Option<String>,
    pub tool_schema: Option<Value>,
    pub sampling_parameters: BTreeMap<String, Value>,
    pub output_format: Option<String>,
    pub retrieval_context_hash: Option<String>,
    pub application_namespace: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct EmbeddingCacheKeyInput {
    pub model: String,
    pub model_version: Option<String>,
    pub input_content_hash: String,
    pub normalization_settings: BTreeMap<String, Value>,
    pub chunking_strategy: String,
    pub dimensions: u32,
    pub application_namespace: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GenerationLeaseStart {
    Hit {
        value: Vec<u8>,
    },
    Stale {
        value: Vec<u8>,
    },
    LeaseGranted {
        lease_token: String,
        stale_value: Option<Vec<u8>>,
    },
    LeaseDenied,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GenerationLeaseAction {
    ReturnCached(Vec<u8>),
    Generate {
        lease_token: String,
        stale_value: Option<Vec<u8>>,
    },
    RetryLater,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GenerationCompletion {
    Completed,
    Failed {
        lease_token: String,
        reason: GenerationCompletionFailure,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GenerationCompletionFailure {
    InvalidLeaseToken,
    ExpiredLease,
    RejectedValue,
    TransportError,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StreamCapture {
    chunks: Vec<Vec<u8>>,
    failed: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CapturedStream {
    pub lease_token: String,
    pub value: Vec<u8>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StreamCaptureError {
    GenerationFailed,
}

impl StreamCapture {
    pub fn new() -> Self {
        Self {
            chunks: Vec::new(),
            failed: false,
        }
    }

    pub fn push_chunk(&mut self, chunk: impl Into<Vec<u8>>) {
        if !self.failed {
            self.chunks.push(chunk.into());
        }
    }

    pub fn mark_failed(&mut self) {
        self.failed = true;
        self.chunks.clear();
    }

    pub fn finish(
        self,
        lease_token: impl Into<String>,
    ) -> Result<CapturedStream, StreamCaptureError> {
        if self.failed {
            return Err(StreamCaptureError::GenerationFailed);
        }

        let value = self.chunks.into_iter().flatten().collect();
        Ok(CapturedStream {
            lease_token: lease_token.into(),
            value,
        })
    }
}

impl Default for StreamCapture {
    fn default() -> Self {
        Self::new()
    }
}

impl EmbeddingCacheKeyInput {
    pub fn new(
        model: impl Into<String>,
        input_content_hash: impl Into<String>,
        chunking_strategy: impl Into<String>,
        dimensions: u32,
        application_namespace: impl Into<String>,
    ) -> Self {
        Self {
            model: model.into(),
            model_version: None,
            input_content_hash: input_content_hash.into(),
            normalization_settings: BTreeMap::new(),
            chunking_strategy: chunking_strategy.into(),
            dimensions,
            application_namespace: application_namespace.into(),
        }
    }
}

impl PromptCacheKeyInput {
    pub fn new(
        provider: impl Into<String>,
        model: impl Into<String>,
        application_namespace: impl Into<String>,
    ) -> Self {
        Self {
            provider: provider.into(),
            model: model.into(),
            model_version: None,
            messages: Vec::new(),
            system_prompt: None,
            tool_schema: None,
            sampling_parameters: BTreeMap::new(),
            output_format: None,
            retrieval_context_hash: None,
            application_namespace: application_namespace.into(),
        }
    }
}

pub fn prompt_cache_key(input: &PromptCacheKeyInput) -> Vec<u8> {
    let normalized = normalize_prompt_input(input);
    let digest = fnv1a_128(&normalized);
    digest_key(PROMPT_KEY_PREFIX, digest)
}

pub fn embedding_cache_key(input: &EmbeddingCacheKeyInput) -> Vec<u8> {
    let normalized = normalize_embedding_input(input);
    let digest = fnv1a_128(&normalized);
    digest_key(EMBEDDING_KEY_PREFIX, digest)
}

pub fn generation_lease_action(start: GenerationLeaseStart) -> GenerationLeaseAction {
    match start {
        GenerationLeaseStart::Hit { value } | GenerationLeaseStart::Stale { value } => {
            GenerationLeaseAction::ReturnCached(value)
        }
        GenerationLeaseStart::LeaseGranted {
            lease_token,
            stale_value,
        } => GenerationLeaseAction::Generate {
            lease_token,
            stale_value,
        },
        GenerationLeaseStart::LeaseDenied => GenerationLeaseAction::RetryLater,
    }
}

pub fn generation_completion_success() -> GenerationCompletion {
    GenerationCompletion::Completed
}

pub fn generation_completion_failure(
    lease_token: impl Into<String>,
    reason: GenerationCompletionFailure,
) -> GenerationCompletion {
    GenerationCompletion::Failed {
        lease_token: lease_token.into(),
        reason,
    }
}

pub fn normalize_prompt_input(input: &PromptCacheKeyInput) -> Vec<u8> {
    let mut out = Vec::new();
    append_bytes(&mut out, b"version", PROMPT_NORMALIZATION_VERSION);
    append_str(&mut out, b"provider", &input.provider);
    append_str(&mut out, b"model", &input.model);
    append_optional_str(&mut out, b"model_version", input.model_version.as_deref());
    append_optional_str(&mut out, b"system_prompt", input.system_prompt.as_deref());
    append_json(&mut out, b"tool_schema", input.tool_schema.as_ref());
    append_json_map(
        &mut out,
        b"sampling_parameters",
        b"sampling.name",
        b"sampling.value",
        &input.sampling_parameters,
    );
    append_optional_str(&mut out, b"output_format", input.output_format.as_deref());
    append_optional_str(
        &mut out,
        b"retrieval_context_hash",
        input.retrieval_context_hash.as_deref(),
    );
    append_str(
        &mut out,
        b"application_namespace",
        &input.application_namespace,
    );
    append_u64(&mut out, input.messages.len() as u64);
    for message in &input.messages {
        append_str(&mut out, b"message.role", &message.role);
        append_optional_str(&mut out, b"message.name", message.name.as_deref());
        append_str(&mut out, b"message.content", &message.content);
    }
    out
}

pub fn normalize_embedding_input(input: &EmbeddingCacheKeyInput) -> Vec<u8> {
    let mut out = Vec::new();
    append_bytes(&mut out, b"version", EMBEDDING_NORMALIZATION_VERSION);
    append_str(&mut out, b"model", &input.model);
    append_optional_str(&mut out, b"model_version", input.model_version.as_deref());
    append_str(&mut out, b"input_content_hash", &input.input_content_hash);
    append_json_map(
        &mut out,
        b"normalization_settings",
        b"normalization.name",
        b"normalization.value",
        &input.normalization_settings,
    );
    append_str(&mut out, b"chunking_strategy", &input.chunking_strategy);
    append_u64(&mut out, u64::from(input.dimensions));
    append_str(
        &mut out,
        b"application_namespace",
        &input.application_namespace,
    );
    out
}

fn append_json_map(
    out: &mut Vec<u8>,
    field: &[u8],
    key_field: &[u8],
    value_field: &[u8],
    values: &BTreeMap<String, Value>,
) {
    append_bytes(out, field, b"map");
    append_u64(out, values.len() as u64);
    for (name, value) in values {
        append_str(out, key_field, name);
        append_bytes(out, value_field, canonical_json(value).as_bytes());
    }
}

fn append_json(out: &mut Vec<u8>, field: &[u8], value: Option<&Value>) {
    match value {
        Some(value) => append_bytes(out, field, canonical_json(value).as_bytes()),
        None => append_bytes(out, field, b"<none>"),
    }
}

fn append_optional_str(out: &mut Vec<u8>, field: &[u8], value: Option<&str>) {
    match value {
        Some(value) => append_str(out, field, value),
        None => append_bytes(out, field, b"<none>"),
    }
}

fn append_str(out: &mut Vec<u8>, field: &[u8], value: &str) {
    append_bytes(out, field, value.as_bytes());
}

fn append_bytes(out: &mut Vec<u8>, field: &[u8], value: &[u8]) {
    append_u64(out, field.len() as u64);
    out.extend_from_slice(field);
    append_u64(out, value.len() as u64);
    out.extend_from_slice(value);
}

fn append_u64(out: &mut Vec<u8>, value: u64) {
    out.extend_from_slice(&value.to_be_bytes());
}

fn canonical_json(value: &Value) -> String {
    match value {
        Value::Null => "null".to_string(),
        Value::Bool(value) => value.to_string(),
        Value::Number(value) => value.to_string(),
        Value::String(value) => {
            serde_json::to_string(value).expect("serializing a JSON string should not fail")
        }
        Value::Array(values) => {
            let items = values
                .iter()
                .map(canonical_json)
                .collect::<Vec<_>>()
                .join(",");
            format!("[{items}]")
        }
        Value::Object(values) => {
            let mut items = values.iter().collect::<Vec<_>>();
            items.sort_by_key(|(key, _)| *key);
            let fields = items
                .into_iter()
                .map(|(key, value)| {
                    let key =
                        serde_json::to_string(key).expect("serializing a JSON key should not fail");
                    format!("{key}:{}", canonical_json(value))
                })
                .collect::<Vec<_>>()
                .join(",");
            format!("{{{fields}}}")
        }
    }
}

fn fnv1a_128(bytes: &[u8]) -> u128 {
    bytes.iter().fold(FNV_OFFSET_BASIS, |hash, byte| {
        (hash ^ u128::from(*byte)).wrapping_mul(FNV_PRIME)
    })
}

fn write_hex_u128(value: u128, out: &mut Vec<u8>) {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    for shift in (0..128).step_by(4).rev() {
        out.push(HEX[((value >> shift) & 0xf) as usize]);
    }
}

fn digest_key(prefix: &[u8], digest: u128) -> Vec<u8> {
    let mut key = prefix.to_vec();
    write_hex_u128(digest, &mut key);
    key
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn base_input() -> PromptCacheKeyInput {
        let mut input = PromptCacheKeyInput::new("openai", "gpt-example", "workspace-a");
        input.model_version = Some("2026-06-01".to_string());
        input.system_prompt = Some("Answer with citations.".to_string());
        input.messages = vec![
            PromptMessage::new("user", "Summarize the release notes."),
            PromptMessage::new("assistant", "Which product?").with_name("cachebot"),
            PromptMessage::new("user", "Cachebox."),
        ];
        input.tool_schema = Some(json!({
            "type": "object",
            "properties": {
                "query": { "type": "string" },
                "limit": { "type": "integer" }
            }
        }));
        input
            .sampling_parameters
            .insert("temperature".to_string(), json!(0.2));
        input
            .sampling_parameters
            .insert("top_p".to_string(), json!(0.95));
        input.output_format = Some("json".to_string());
        input.retrieval_context_hash = Some("sha256:abc123".to_string());
        input
    }

    fn base_embedding_input() -> EmbeddingCacheKeyInput {
        let mut input = EmbeddingCacheKeyInput::new(
            "text-embedding-example",
            "sha256:contentabc",
            "markdown:v1:512:64",
            1536,
            "workspace-a",
        );
        input.model_version = Some("2026-06-01".to_string());
        input
            .normalization_settings
            .insert("case_fold".to_string(), json!(false));
        input
            .normalization_settings
            .insert("unicode".to_string(), json!("nfc"));
        input
    }

    #[test]
    fn prompt_keys_are_deterministic_for_equivalent_inputs() {
        let left = base_input();
        let mut right = base_input();
        right.tool_schema = Some(json!({
            "properties": {
                "limit": { "type": "integer" },
                "query": { "type": "string" }
            },
            "type": "object"
        }));

        assert_eq!(prompt_cache_key(&left), prompt_cache_key(&right));
        assert_eq!(
            normalize_prompt_input(&left),
            normalize_prompt_input(&right)
        );
    }

    #[test]
    fn prompt_keys_change_for_meaningful_input_changes() {
        let original = prompt_cache_key(&base_input());

        let mut changed_prompt = base_input();
        changed_prompt.messages[0].content = "Summarize the changelog.".to_string();
        assert_ne!(original, prompt_cache_key(&changed_prompt));

        let mut changed_model = base_input();
        changed_model.model = "other-model".to_string();
        assert_ne!(original, prompt_cache_key(&changed_model));

        let mut changed_tool = base_input();
        changed_tool.tool_schema = Some(json!({"type": "object", "required": ["query"]}));
        assert_ne!(original, prompt_cache_key(&changed_tool));

        let mut changed_sampling = base_input();
        changed_sampling
            .sampling_parameters
            .insert("temperature".to_string(), json!(0.8));
        assert_ne!(original, prompt_cache_key(&changed_sampling));

        let mut changed_retrieval = base_input();
        changed_retrieval.retrieval_context_hash = Some("sha256:def456".to_string());
        assert_ne!(original, prompt_cache_key(&changed_retrieval));
    }

    #[test]
    fn prompt_key_output_is_binary_safe_ascii() {
        let mut input = base_input();
        input.messages.push(PromptMessage::new(
            "user",
            "Unicode survives normalization: Zażółć gęślą jaźń 🚀",
        ));

        let key = prompt_cache_key(&input);

        assert!(key.starts_with(PROMPT_KEY_PREFIX));
        assert_eq!(key.len(), PROMPT_KEY_PREFIX.len() + 32);
        assert!(key.iter().all(u8::is_ascii));
    }

    #[test]
    fn message_order_affects_prompt_keys() {
        let original = prompt_cache_key(&base_input());
        let mut reordered = base_input();
        reordered.messages.swap(0, 1);

        assert_ne!(original, prompt_cache_key(&reordered));
    }

    #[test]
    fn missing_optional_fields_are_distinct_from_empty_values() {
        let mut missing = PromptCacheKeyInput::new("openai", "gpt-example", "workspace-a");
        missing.messages.push(PromptMessage::new("user", "hello"));

        let mut empty = missing.clone();
        empty.system_prompt = Some(String::new());

        assert_ne!(prompt_cache_key(&missing), prompt_cache_key(&empty));
    }

    #[test]
    fn embedding_keys_are_deterministic_for_equivalent_inputs() {
        let left = base_embedding_input();
        let mut right = EmbeddingCacheKeyInput::new(
            "text-embedding-example",
            "sha256:contentabc",
            "markdown:v1:512:64",
            1536,
            "workspace-a",
        );
        right.model_version = Some("2026-06-01".to_string());
        right
            .normalization_settings
            .insert("unicode".to_string(), json!("nfc"));
        right
            .normalization_settings
            .insert("case_fold".to_string(), json!(false));

        assert_eq!(embedding_cache_key(&left), embedding_cache_key(&right));
        assert_eq!(
            normalize_embedding_input(&left),
            normalize_embedding_input(&right)
        );
    }

    #[test]
    fn embedding_keys_change_for_model_input_chunking_and_dimensions() {
        let original = embedding_cache_key(&base_embedding_input());

        let mut changed_model = base_embedding_input();
        changed_model.model = "other-embedding-model".to_string();
        assert_ne!(original, embedding_cache_key(&changed_model));

        let mut changed_input = base_embedding_input();
        changed_input.input_content_hash = "sha256:contentdef".to_string();
        assert_ne!(original, embedding_cache_key(&changed_input));

        let mut changed_normalization = base_embedding_input();
        changed_normalization
            .normalization_settings
            .insert("case_fold".to_string(), json!(true));
        assert_ne!(original, embedding_cache_key(&changed_normalization));

        let mut changed_chunking = base_embedding_input();
        changed_chunking.chunking_strategy = "markdown:v2:512:64".to_string();
        assert_ne!(original, embedding_cache_key(&changed_chunking));

        let mut changed_dimensions = base_embedding_input();
        changed_dimensions.dimensions = 768;
        assert_ne!(original, embedding_cache_key(&changed_dimensions));
    }

    #[test]
    fn embedding_key_output_is_binary_safe_ascii() {
        let key = embedding_cache_key(&base_embedding_input());

        assert!(key.starts_with(EMBEDDING_KEY_PREFIX));
        assert_eq!(key.len(), EMBEDDING_KEY_PREFIX.len() + 32);
        assert!(key.iter().all(u8::is_ascii));
    }

    #[test]
    fn generation_lease_helper_returns_fresh_hits() {
        let action = generation_lease_action(GenerationLeaseStart::Hit {
            value: b"fresh".to_vec(),
        });

        assert_eq!(
            action,
            GenerationLeaseAction::ReturnCached(b"fresh".to_vec())
        );
    }

    #[test]
    fn generation_lease_helper_returns_stale_values() {
        let action = generation_lease_action(GenerationLeaseStart::Stale {
            value: b"stale".to_vec(),
        });

        assert_eq!(
            action,
            GenerationLeaseAction::ReturnCached(b"stale".to_vec())
        );
    }

    #[test]
    fn generation_lease_helper_generates_only_when_granted() {
        let action = generation_lease_action(GenerationLeaseStart::LeaseGranted {
            lease_token: "lease-1".to_string(),
            stale_value: Some(b"old".to_vec()),
        });

        assert_eq!(
            action,
            GenerationLeaseAction::Generate {
                lease_token: "lease-1".to_string(),
                stale_value: Some(b"old".to_vec())
            }
        );
    }

    #[test]
    fn generation_lease_helper_retries_later_when_denied() {
        let action = generation_lease_action(GenerationLeaseStart::LeaseDenied);

        assert_eq!(action, GenerationLeaseAction::RetryLater);
    }

    #[test]
    fn generation_completion_failure_preserves_token_and_reason() {
        let failure = generation_completion_failure(
            "lease-1",
            GenerationCompletionFailure::InvalidLeaseToken,
        );

        assert_eq!(
            failure,
            GenerationCompletion::Failed {
                lease_token: "lease-1".to_string(),
                reason: GenerationCompletionFailure::InvalidLeaseToken
            }
        );
        assert_eq!(
            generation_completion_success(),
            GenerationCompletion::Completed
        );
    }

    #[test]
    fn stream_capture_commits_concatenated_raw_chunks_after_success() {
        let mut capture = StreamCapture::new();
        capture.push_chunk(b"hello ".to_vec());
        capture.push_chunk(vec![0, 1, 2]);
        capture.push_chunk(b" world".to_vec());

        let captured = capture.finish("lease-1").expect("capture should finish");

        assert_eq!(
            captured,
            CapturedStream {
                lease_token: "lease-1".to_string(),
                value: b"hello \x00\x01\x02 world".to_vec()
            }
        );
    }

    #[test]
    fn stream_capture_does_not_publish_failed_generations() {
        let mut capture = StreamCapture::new();
        capture.push_chunk(b"partial".to_vec());
        capture.mark_failed();
        capture.push_chunk(b" ignored".to_vec());

        assert_eq!(
            capture.finish("lease-1"),
            Err(StreamCaptureError::GenerationFailed)
        );
    }

    #[test]
    fn stream_capture_replay_bytes_are_deterministic() {
        let mut first = StreamCapture::new();
        first.push_chunk(b"a".to_vec());
        first.push_chunk(b"b".to_vec());

        let mut second = StreamCapture::new();
        second.push_chunk(b"ab".to_vec());

        assert_eq!(
            first.finish("lease-1").expect("first capture").value,
            second.finish("lease-2").expect("second capture").value
        );
    }
}
