use std::env;
use std::process::ExitCode;

use cachebox::config::{Config, ConfigError, help_text};
use cachebox::server;

#[tokio::main]
async fn main() -> ExitCode {
    let program_name = env::args().next().unwrap_or_else(|| "cachebox".to_string());

    match Config::from_args(env::args().skip(1)) {
        Ok(config) => {
            let report = server::startup_report(&config);
            println!("bind_addr={}", report.bind_addr);
            println!(
                "native_bind_addr={}",
                report.native_bind_addr.as_deref().unwrap_or("disabled")
            );
            println!(
                "native_unix_socket={}",
                report.native_unix_socket.as_deref().unwrap_or("disabled")
            );
            println!("max_body_bytes={}", report.max_body_bytes);
            println!("max_memory_bytes={}", report.max_memory_bytes);
            println!("max_value_bytes={}", report.max_value_bytes);
            println!("cleanup_interval_ms={}", report.cleanup_interval_ms);
            println!(
                "cleanup_max_entries_per_tick={}",
                report.cleanup_max_entries_per_tick
            );
            match server::run(config).await {
                Ok(()) => ExitCode::SUCCESS,
                Err(error) => {
                    eprintln!("cachebox: failed to run server: {error}");
                    ExitCode::FAILURE
                }
            }
        }
        Err(ConfigError::HelpRequested) => {
            print!("{}", help_text(&program_name));
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("cachebox: {error}");
            eprintln!("try '{program_name} --help'");
            ExitCode::from(2)
        }
    }
}
