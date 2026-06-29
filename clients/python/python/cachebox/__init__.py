"""Official Cachebox Python client package.

This package is intentionally pure Python by default.
"""

from . import protocol
from .async_client import AsyncClient, AsyncClientPool
from .client import (
    Client,
    ClientError,
    ClientPool,
    ConnectionClosed,
    ServerError,
    UnexpectedResponse,
)
from .cache import (
    AsyncCacheBackend,
    AsyncCachebox,
    CacheBackend,
    Cachebox,
    DogpilePolicy,
    DogpileTimeoutError,
)
from .keys import (
    KeyBuildError,
    KeyOptions,
    build_custom_key,
    build_function_key,
    build_template_key,
    make_metadata,
)
from .serde import (
    BytesSerializer,
    DecodeError,
    EncodeError,
    JsonSerializer,
    PickleSerializer,
    SerializationError,
    Serializer,
)

__version__ = "0.2.0"

__all__ = [
    "__version__",
    "protocol",
    "AsyncClient",
    "AsyncClientPool",
    "Client",
    "ClientError",
    "ClientPool",
    "ConnectionClosed",
    "ServerError",
    "UnexpectedResponse",
    "AsyncCacheBackend",
    "AsyncCachebox",
    "BytesSerializer",
    "CacheBackend",
    "Cachebox",
    "DecodeError",
    "DogpilePolicy",
    "DogpileTimeoutError",
    "EncodeError",
    "JsonSerializer",
    "KeyBuildError",
    "KeyOptions",
    "PickleSerializer",
    "SerializationError",
    "Serializer",
    "build_custom_key",
    "build_function_key",
    "build_template_key",
    "make_metadata",
]
