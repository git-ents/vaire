mod cli;
mod exe;
mod render;

use std::process::ExitCode;

use clap::Parser;

/// Parse command-line arguments and execute the selected operation.
fn main() -> ExitCode {
    match exe::run(cli::Cli::parse().command) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => exe::exit_code(error),
    }
}
