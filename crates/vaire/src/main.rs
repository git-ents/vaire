mod cli;
mod exe;
mod render;

use std::process::ExitCode;

use clap::Parser;

use cli::Cli;

/// Parse command-line arguments and execute the selected operation.
fn main() -> ExitCode {
    let Cli { command, color } = cli::Cli::parse();
    match exe::run(command, color) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => exe::exit_code(error),
    }
}
