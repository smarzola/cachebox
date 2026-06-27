//! AI-oriented client helpers.
//!
//! These helpers are intentionally provider-neutral. They build deterministic
//! cache keys from structured request metadata, but they do not interpret prompt
//! semantics or call model providers.

use std::collections::BTreeMap;

use serde_json::Value;

const PROMPT_KEY_PREFIX: &[u8] = b"ai:prompt:v1:";
const NORMALIZATION_VERSION: &[u8] = b"cachebox.ai.prompt.v1";
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
    let mut key = PROMPT_KEY_PREFIX.to_vec();
    write_hex_u128(digest, &mut key);
    key
}

pub fn normalize_prompt_input(input: &PromptCacheKeyInput) -> Vec<u8> {
    let mut out = Vec::new();
    append_bytes(&mut out, b"version", NORMALIZATION_VERSION);
    append_str(&mut out, b"provider", &input.provider);
    append_str(&mut out, b"model", &input.model);
    append_optional_str(&mut out, b"model_version", input.model_version.as_deref());
    append_optional_str(&mut out, b"system_prompt", input.system_prompt.as_deref());
    append_json(&mut out, b"tool_schema", input.tool_schema.as_ref());
    append_sampling_parameters(&mut out, &input.sampling_parameters);
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

fn append_sampling_parameters(out: &mut Vec<u8>, parameters: &BTreeMap<String, Value>) {
    append_bytes(out, b"sampling_parameters", b"map");
    append_u64(out, parameters.len() as u64);
    for (name, value) in parameters {
        append_str(out, b"sampling.name", name);
        append_bytes(out, b"sampling.value", canonical_json(value).as_bytes());
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
}
