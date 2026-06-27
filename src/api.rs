//! Shared cache metadata and admin HTTP route constants.

pub const HEALTH_ROUTE: &str = "/healthz";
pub const METRICS_ROUTE: &str = "/metrics";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContentType {
    OctetStream,
    Other,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Ttl {
    pub milliseconds: u64,
}
