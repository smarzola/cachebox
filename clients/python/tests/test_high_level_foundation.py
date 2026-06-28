import pytest

from cachebox import (
    BytesSerializer,
    EncodeError,
    JsonSerializer,
    KeyBuildError,
    PickleSerializer,
    Serializer,
    build_custom_key,
    build_function_key,
    build_template_key,
    make_metadata,
    protocol,
)


def sample_function(user_id, limit=10, *, active=True):
    return user_id, limit, active


def test_bytes_serializer_round_trips_bytes_like_values():
    serializer = BytesSerializer()

    assert serializer.encode(b"value") == b"value"
    assert serializer.encode(bytearray(b"value")) == b"value"
    assert serializer.encode(memoryview(b"value")) == b"value"
    assert serializer.decode(b"value") == b"value"


def test_bytes_serializer_rejects_non_bytes_values():
    serializer = BytesSerializer()

    with pytest.raises(EncodeError):
        serializer.encode("value")


def test_json_serializer_uses_deterministic_utf8_json():
    serializer = JsonSerializer()

    encoded = serializer.encode({"b": 2, "a": ["snow", True]})

    assert encoded == '{"a":["snow",true],"b":2}'.encode()
    assert serializer.decode(encoded) == {"a": ["snow", True], "b": 2}


def test_json_serializer_rejects_unsupported_values():
    serializer = JsonSerializer()

    with pytest.raises(EncodeError):
        serializer.encode({b"bytes"})

    with pytest.raises(EncodeError):
        serializer.encode(float("nan"))


def test_pickle_serializer_is_explicit_and_round_trips_python_values():
    serializer = PickleSerializer()

    payload = serializer.encode({"values": (1, 2, 3)})

    assert serializer.decode(payload) == {"values": (1, 2, 3)}


def test_function_keys_are_stable_across_call_shapes_and_defaults():
    implicit_default = build_function_key(sample_function, 42)
    explicit_default = build_function_key(sample_function, 42, limit=10, active=True)
    keyword_order = build_function_key(
        sample_function,
        user_id=42,
        active=True,
        limit=10,
    )

    assert implicit_default == explicit_default == keyword_order
    assert b"sample_function" in implicit_default
    assert b'"limit",10' in implicit_default


def test_function_keys_include_prefix_version_and_bytes_arguments():
    key = build_function_key(
        sample_function,
        b"user-42",
        prefix="users",
        version=2,
    )

    assert key.startswith(b"users:2:")
    assert b"__cachebox_type__" in key
    assert b"dXNlci00Mg==" in key


def test_function_key_rejects_unsupported_argument_values():
    class Unstable:
        pass

    with pytest.raises(KeyBuildError):
        build_function_key(sample_function, Unstable())

    with pytest.raises(KeyBuildError):
        build_function_key(sample_function, float("inf"))


def test_template_key_uses_explicit_values():
    key = build_template_key(
        "user:{user_id}:active:{active}",
        prefix="api",
        version="v1",
        user_id=42,
        active=True,
    )

    assert key == b"api:v1:user:42:active:true"


def test_custom_key_function_result_can_be_prefixed():
    key = build_custom_key({"tenant": "acme", "id": 7}, prefix="lookup")

    assert key == b'lookup:[["id",7],["tenant","acme"]]'


def test_make_metadata_maps_high_level_options_to_protocol_metadata():
    metadata = make_metadata(
        ttl_ms=60_000,
        stale_ttl_ms=30_000,
        tags=("users", "profile"),
        cost=5,
        content_type=JsonSerializer.content_type,
    )

    assert metadata == protocol.Metadata(
        ttl=protocol.Ttl(60_000),
        stale_ttl=protocol.Ttl(30_000),
        cost=5,
        tags=("users", "profile"),
        content_type=protocol.ContentType.OTHER,
    )


def test_serializer_protocol_accepts_builtin_serializers():
    assert isinstance(BytesSerializer(), Serializer)
    assert isinstance(JsonSerializer(), Serializer)
    assert isinstance(PickleSerializer(), Serializer)
