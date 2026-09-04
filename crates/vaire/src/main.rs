use std::process::ExitCode;

use vaire::check::Violation;
use vaire::emit::emit;
use vaire::extract::extract;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match run(&args) {
        Ok(()) => ExitCode::SUCCESS,
        Err(Fail::Violations(violations)) => {
            for v in violations {
                eprintln!("{v}");
            }
            ExitCode::FAILURE
        }
        Err(Fail::Message(message)) => {
            eprintln!("vaire: {message}");
            ExitCode::FAILURE
        }
    }
}

enum Fail {
    Message(String),
    Violations(Vec<Violation>),
}

fn run(args: &[String]) -> Result<(), Fail> {
    match args.split_first() {
        Some((cmd, files)) if cmd == "extract" => {
            let mut all = Vec::new();
            for file in files {
                let source =
                    std::fs::read_to_string(file).map_err(|e| Fail::Message(e.to_string()))?;
                all.extend(extract(file, &source).map_err(|e| Fail::Message(e.to_string()))?);
            }
            let json =
                serde_json::to_string_pretty(&all).map_err(|e| Fail::Message(e.to_string()))?;
            println!("{json}");
            Ok(())
        }
        Some((cmd, rest)) if cmd == "emit" => {
            let [json, file] = rest else {
                return Err(Fail::Message("usage: vaire emit <json> <file>".to_owned()));
            };
            let json = std::fs::read_to_string(json).map_err(|e| Fail::Message(e.to_string()))?;
            emit(&json, file).map_err(|e| Fail::Message(e.to_string()))
        }
        Some((cmd, files)) if cmd == "check" => {
            let violations =
                vaire::check::check(files).map_err(|e| Fail::Message(e.to_string()))?;
            if violations.is_empty() {
                Ok(())
            } else {
                Err(Fail::Violations(violations))
            }
        }
        _ => Err(Fail::Message(
            "usage: vaire extract <file...> | emit <json> <file> | check <file...>".to_owned(),
        )),
    }
}
