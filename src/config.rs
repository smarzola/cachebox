use std::fmt;
use std::net::SocketAddr;

const DEFAULT_BIND_ADDR: &str = "127.0.0.1:7400";
const DEFAULT_MAX_BODY_BYTES: usize = 8 * 1024 * 1024;
const DEFAULT_MAX_MEMORY_BYTES: usize = 64 * 1024 * 1024;
const DEFAULT_MAX_VALUE_BYTES: usize = 8 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Config {
    pub bind_addr: SocketAddr,
    pub max_body_bytes: usize,
    pub max_memory_bytes: usize,
    pub max_value_bytes: usize,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            bind_addr: DEFAULT_BIND_ADDR
                .parse()
                .expect("default bind address must be valid"),
            max_body_bytes: DEFAULT_MAX_BODY_BYTES,
            max_memory_bytes: DEFAULT_MAX_MEMORY_BYTES,
            max_value_bytes: DEFAULT_MAX_VALUE_BYTES,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConfigError {
    HelpRequested,
    MissingValue { flag: String },
    UnknownArgument { argument: String },
    InvalidBindAddress { value: String },
    InvalidMaxBodyBytes { value: String },
    InvalidMaxMemoryBytes { value: String },
    InvalidMaxValueBytes { value: String },
}

impl fmt::Display for ConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::HelpRequested => write!(f, "help requested"),
            Self::MissingValue { flag } => write!(f, "missing value for {flag}"),
            Self::UnknownArgument { argument } => write!(f, "unknown argument: {argument}"),
            Self::InvalidBindAddress { value } => {
                write!(f, "invalid bind address: {value}")
            }
            Self::InvalidMaxBodyBytes { value } => {
                write!(f, "invalid max body byte limit: {value}")
            }
            Self::InvalidMaxMemoryBytes { value } => {
                write!(f, "invalid max memory byte limit: {value}")
            }
            Self::InvalidMaxValueBytes { value } => {
                write!(f, "invalid max value byte limit: {value}")
            }
        }
    }
}

impl std::error::Error for ConfigError {}

impl Config {
    pub fn from_args<I, S>(args: I) -> Result<Self, ConfigError>
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        let mut config = Self::default();
        let mut args = args.into_iter().map(Into::into);

        while let Some(argument) = args.next() {
            match argument.as_str() {
                "-h" | "--help" => return Err(ConfigError::HelpRequested),
                "--bind" => {
                    let value = args.next().ok_or_else(|| ConfigError::MissingValue {
                        flag: argument.clone(),
                    })?;
                    config.bind_addr =
                        value.parse().map_err(|_| ConfigError::InvalidBindAddress {
                            value: value.clone(),
                        })?;
                }
                "--max-body-bytes" => {
                    config.max_body_bytes = parse_nonzero_usize(&argument, args.next(), |value| {
                        ConfigError::InvalidMaxBodyBytes { value }
                    })?;
                }
                "--max-memory-bytes" => {
                    config.max_memory_bytes =
                        parse_nonzero_usize(&argument, args.next(), |value| {
                            ConfigError::InvalidMaxMemoryBytes { value }
                        })?;
                }
                "--max-value-bytes" => {
                    config.max_value_bytes =
                        parse_nonzero_usize(&argument, args.next(), |value| {
                            ConfigError::InvalidMaxValueBytes { value }
                        })?;
                }
                _ => return Err(ConfigError::UnknownArgument { argument }),
            }
        }

        Ok(config)
    }
}

fn parse_nonzero_usize(
    flag: &str,
    value: Option<String>,
    error: impl Fn(String) -> ConfigError,
) -> Result<usize, ConfigError> {
    let value = value.ok_or_else(|| ConfigError::MissingValue {
        flag: flag.to_string(),
    })?;
    let parsed = value.parse::<usize>().map_err(|_| error(value.clone()))?;
    if parsed == 0 {
        return Err(error(value));
    }
    Ok(parsed)
}

pub fn help_text(program_name: &str) -> String {
    format!(
        "\
Cachebox cache server

Usage:
  {program_name} [--bind <addr:port>] [--max-body-bytes <bytes>] [--max-memory-bytes <bytes>] [--max-value-bytes <bytes>]
  {program_name} --help

Options:
  --bind <addr:port>  Address for the HTTP server to bind.
                      Default: {DEFAULT_BIND_ADDR}
  --max-body-bytes    Maximum accepted request body size.
                      Default: {DEFAULT_MAX_BODY_BYTES}
  --max-memory-bytes  Maximum estimated in-memory cache size.
                      Default: {DEFAULT_MAX_MEMORY_BYTES}
  --max-value-bytes   Maximum single cached value size.
                      Default: {DEFAULT_MAX_VALUE_BYTES}
  -h, --help          Print this help text.
"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uses_default_bind_address() {
        let config = Config::from_args(Vec::<String>::new()).expect("default config");

        assert_eq!(config.bind_addr.to_string(), DEFAULT_BIND_ADDR);
        assert_eq!(config.max_body_bytes, DEFAULT_MAX_BODY_BYTES);
        assert_eq!(config.max_memory_bytes, DEFAULT_MAX_MEMORY_BYTES);
        assert_eq!(config.max_value_bytes, DEFAULT_MAX_VALUE_BYTES);
    }

    #[test]
    fn parses_bind_address() {
        let config = Config::from_args(["--bind", "0.0.0.0:9000"]).expect("custom config");

        assert_eq!(config.bind_addr.to_string(), "0.0.0.0:9000");
    }

    #[test]
    fn reports_missing_bind_value() {
        let error = Config::from_args(["--bind"]).expect_err("missing value should fail");

        assert_eq!(
            error,
            ConfigError::MissingValue {
                flag: "--bind".to_string()
            }
        );
    }

    #[test]
    fn reports_invalid_bind_address() {
        let error = Config::from_args(["--bind", "not-an-address"]).expect_err("invalid address");

        assert_eq!(
            error,
            ConfigError::InvalidBindAddress {
                value: "not-an-address".to_string()
            }
        );
    }

    #[test]
    fn parses_max_body_bytes() {
        let config = Config::from_args([
            "--max-body-bytes",
            "1024",
            "--max-memory-bytes",
            "2048",
            "--max-value-bytes",
            "512",
        ])
        .expect("custom config");

        assert_eq!(config.max_body_bytes, 1024);
        assert_eq!(config.max_memory_bytes, 2048);
        assert_eq!(config.max_value_bytes, 512);
    }

    #[test]
    fn rejects_invalid_max_body_bytes() {
        let error =
            Config::from_args(["--max-body-bytes", "0"]).expect_err("zero body limit should fail");

        assert_eq!(
            error,
            ConfigError::InvalidMaxBodyBytes {
                value: "0".to_string()
            }
        );
    }

    #[test]
    fn rejects_invalid_memory_limits() {
        let memory_error = Config::from_args(["--max-memory-bytes", "nope"])
            .expect_err("invalid memory limit should fail");
        assert_eq!(
            memory_error,
            ConfigError::InvalidMaxMemoryBytes {
                value: "nope".to_string()
            }
        );

        let value_error = Config::from_args(["--max-value-bytes", "0"])
            .expect_err("invalid value limit should fail");
        assert_eq!(
            value_error,
            ConfigError::InvalidMaxValueBytes {
                value: "0".to_string()
            }
        );
    }

    #[test]
    fn reports_unknown_argument() {
        let error = Config::from_args(["--verbose"]).expect_err("unknown arg should fail");

        assert_eq!(
            error,
            ConfigError::UnknownArgument {
                argument: "--verbose".to_string()
            }
        );
    }

    #[test]
    fn reports_help_request() {
        let error = Config::from_args(["--help"]).expect_err("help should short-circuit");

        assert_eq!(error, ConfigError::HelpRequested);
    }
}
