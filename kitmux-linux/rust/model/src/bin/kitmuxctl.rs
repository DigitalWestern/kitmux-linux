use kitmux_model::{CliParseError, parse_cli, send_control_request};
use std::collections::HashMap;
use std::env;
use std::process::ExitCode;

fn main() -> ExitCode {
    let environment: HashMap<String, String> = env::vars().collect();
    let invocation = match parse_cli(env::args().skip(1), &environment) {
        Ok(invocation) => invocation,
        Err(error @ (CliParseError::Help | CliParseError::Version)) => {
            println!("{error}");
            return ExitCode::SUCCESS;
        }
        Err(error) => {
            eprintln!("{error}");
            return ExitCode::from(2);
        }
    };
    let response = match send_control_request(&invocation.socket, &invocation.request) {
        Ok(response) => response,
        Err(error) => {
            eprintln!(
                "kitmuxctl: cannot reach {}: {error}. Start Kitmux first, or set KITMUX_SOCKET_PATH.",
                invocation.socket.path().display()
            );
            return ExitCode::from(1);
        }
    };
    if invocation.json {
        match serde_json::to_string_pretty(&response) {
            Ok(value) => println!("{value}"),
            Err(error) => {
                eprintln!("kitmuxctl: could not encode response: {error}");
                return ExitCode::from(1);
            }
        }
    } else if response.ok {
        if let Some(result) = response.result {
            println!(
                "{}",
                serde_json::to_string_pretty(&result).unwrap_or_default()
            );
        } else {
            println!("ok");
        }
    } else if let Some(error) = response.error {
        eprintln!("kitmuxctl: {}: {}", error.code, error.message);
        return ExitCode::from(1);
    } else {
        eprintln!("kitmuxctl: server returned an invalid error response");
        return ExitCode::from(1);
    }
    if response.ok {
        ExitCode::SUCCESS
    } else {
        ExitCode::from(1)
    }
}
