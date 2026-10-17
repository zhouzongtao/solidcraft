//! The SolidCraft command line (`solidcraft_cli::run`).
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]
#![forbid(unsafe_code)]

fn main() -> std::process::ExitCode {
    solidcraft_cli::run()
}
