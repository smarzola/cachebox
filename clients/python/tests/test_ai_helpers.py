from cachebox import ai_embedding_cache_key, ai_prompt_cache_key


def prompt_input(**overrides):
    values = {
        "provider": "openai",
        "model": "gpt-example",
        "application_namespace": "workspace-a",
        "messages": [
            {"role": "user", "content": "Summarize the release notes."},
            {
                "role": "assistant",
                "content": "Which product?",
                "name": "cachebot",
            },
            {"role": "user", "content": "Cachebox."},
        ],
        "model_version": "2026-06-01",
        "system_prompt": "Answer with citations.",
        "tool_schema": {
            "type": "object",
            "properties": {
                "query": {"type": "string"},
                "limit": {"type": "integer"},
            },
        },
        "sampling_parameters": {"temperature": 0.2, "top_p": 0.95},
        "output_format": "json",
        "retrieval_context_hash": "sha256:abc123",
    }
    values.update(overrides)
    return values


def test_prompt_cache_key_matches_rust_fixture():
    assert (
        ai_prompt_cache_key(**prompt_input())
        == b"ai:prompt:v1:397abdc764b1d7b104f36d63678db86b"
    )


def test_prompt_cache_key_uses_rust_canonical_json_rules():
    reordered = prompt_input(
        tool_schema={
            "properties": {
                "limit": {"type": "integer"},
                "query": {"type": "string"},
            },
            "type": "object",
        },
        sampling_parameters={"top_p": 0.95, "temperature": 0.2},
    )

    assert ai_prompt_cache_key(**prompt_input()) == ai_prompt_cache_key(**reordered)


def test_embedding_cache_key_matches_rust_fixture():
    assert (
        ai_embedding_cache_key(
            "text-embedding-example",
            "sha256:contentabc",
            "markdown:v1:512:64",
            1536,
            "workspace-a",
            model_version="2026-06-01",
            normalization_settings={"unicode": "nfc", "case_fold": False},
        )
        == b"ai:embedding:v1:8bc8936b1586f40aaf06445703a5b8c5"
    )


def test_embedding_cache_key_changes_for_dimensions():
    base = ai_embedding_cache_key(
        "text-embedding-example",
        "sha256:contentabc",
        "markdown:v1:512:64",
        1536,
        "workspace-a",
    )
    changed = ai_embedding_cache_key(
        "text-embedding-example",
        "sha256:contentabc",
        "markdown:v1:512:64",
        768,
        "workspace-a",
    )

    assert base != changed
