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

__version__ = "0.1.0"

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
]
