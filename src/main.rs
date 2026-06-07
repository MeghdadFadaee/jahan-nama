#![cfg_attr(windows, windows_subsystem = "windows")]

mod gui;

use std::env;
use std::path::{Path, PathBuf};
use std::process;

use mci_client::format::{error_json, megabytes_label, unused_json};
use mci_client::{DotEnvStore, MciError, MciInternetClient, Result};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Command {
    Gui,
    Json,
    Raw,
    Test,
    Unused,
    Help,
}

#[derive(Debug)]
struct Args {
    command: Command,
    env_path: PathBuf,
    interval_seconds: Option<u64>,
}

fn main() {
    let args = match parse_args(env::args().skip(1)) {
        Ok(args) => args,
        Err(message) => {
            eprintln!("{message}\n");
            print_usage();
            process::exit(2);
        }
    };

    if args.command == Command::Help {
        print_usage();
        return;
    }

    if args.command == Command::Json {
        if let Err(error) = run_json(&args.env_path) {
            println!("{}", error_json(&error.to_string()));
            process::exit(1);
        }
        return;
    }

    if let Err(error) = run(args) {
        eprintln!("Error: {error}");
        process::exit(1);
    }
}

fn run(args: Args) -> Result<()> {
    match args.command {
        Command::Gui => {
            let interval = interval_seconds(&args.env_path, args.interval_seconds);
            gui::run_gui(args.env_path, interval)
        }
        Command::Raw => {
            let mut client = MciInternetClient::new(&args.env_path)?;
            let payload = client.get_packages_response()?;
            println!(
                "{}",
                serde_json::to_string_pretty(&payload).map_err(json_error)?
            );
            Ok(())
        }
        Command::Test => run_test(&args.env_path),
        Command::Unused => {
            let mut client = MciInternetClient::new(&args.env_path)?;
            let amounts = client.get_unused_amounts_bytes()?;
            for amount in amounts {
                println!("{}", megabytes_label(amount));
            }
            Ok(())
        }
        Command::Json | Command::Help => Ok(()),
    }
}

fn run_json(env_path: &Path) -> Result<()> {
    let mut client = MciInternetClient::new(env_path)?;
    let amounts = client.get_unused_amounts_bytes()?;
    println!("{}", unused_json(&amounts));
    Ok(())
}

fn run_test(env_path: &Path) -> Result<()> {
    println!("=== MCI Internet Client Test ===");

    let mut client = MciInternetClient::new(env_path)?;
    println!("\n[1] Checking existing token status...");
    println!("Access token exists: {}", client.access_token().is_some());
    println!("Refresh token exists: {}", client.refresh_token().is_some());

    println!("\n[2] Ensuring valid token...");
    let token = client.ensure_token(false)?;
    println!("Token acquired: {}...", token_preview(&token));

    println!("\n[3] Fetching packages details...");
    let packages = client.get_packages_response()?;
    println!("Packages fetched successfully.");

    println!("\n--- Raw Response (truncated) ---");
    let raw = serde_json::to_string_pretty(&packages).map_err(json_error)?;
    println!("{}...", raw.chars().take(300).collect::<String>());

    println!("\n[4] Extracting unusedAmount values...");
    let amounts = mci_client::collect_unused_amounts(&packages);
    if amounts.is_empty() {
        println!("No unusedAmount found!");
    } else {
        println!("Found {} entries:", amounts.len());
        for (index, value) in amounts.iter().enumerate() {
            println!(
                "  {}. {} bytes (~{:.2} GB)",
                index + 1,
                value,
                *value as f64 / 1024.0 / 1024.0 / 1024.0
            );
        }

        let total: i64 = amounts.iter().sum();
        println!(
            "\nTotal unused: {total} bytes (~{:.2} GB)",
            total as f64 / 1024.0 / 1024.0 / 1024.0
        );
    }

    println!("\n=== TEST SUCCESS ===");
    Ok(())
}

fn parse_args(args: impl IntoIterator<Item = String>) -> std::result::Result<Args, String> {
    let mut command = None;
    let mut env_path = PathBuf::from(".env");
    let mut interval_seconds = None;
    let mut iter = args.into_iter();

    while let Some(arg) = iter.next() {
        match arg.as_str() {
            "-h" | "--help" => command = Some(Command::Help),
            "-e" | "--env" => {
                let value = iter
                    .next()
                    .ok_or_else(|| format!("{arg} requires a path value"))?;
                env_path = PathBuf::from(value);
            }
            "-i" | "--interval" => {
                let value = iter
                    .next()
                    .ok_or_else(|| format!("{arg} requires a seconds value"))?;
                interval_seconds = Some(
                    value
                        .parse()
                        .map_err(|_| format!("{arg} requires a positive integer"))?,
                );
            }
            "gui" => set_command(&mut command, Command::Gui)?,
            "json" => set_command(&mut command, Command::Json)?,
            "raw" => set_command(&mut command, Command::Raw)?,
            "test" => set_command(&mut command, Command::Test)?,
            "unused" => set_command(&mut command, Command::Unused)?,
            unknown if unknown.starts_with('-') => {
                return Err(format!("Unknown option: {unknown}"));
            }
            unknown => return Err(format!("Unknown command: {unknown}")),
        }
    }

    Ok(Args {
        command: command.unwrap_or(Command::Gui),
        env_path,
        interval_seconds,
    })
}

fn set_command(current: &mut Option<Command>, command: Command) -> std::result::Result<(), String> {
    if current.is_some() {
        return Err("Only one command can be provided".to_owned());
    }
    *current = Some(command);
    Ok(())
}

fn interval_seconds(env_path: &Path, override_value: Option<u64>) -> u64 {
    override_value
        .or_else(|| {
            DotEnvStore::new(env_path).ok().and_then(|env| {
                env.get("PULL_INTERVAL_SECONDS")
                    .and_then(|value| value.parse().ok())
            })
        })
        .filter(|value| *value > 0)
        .unwrap_or(10)
}

fn token_preview(token: &str) -> String {
    token.chars().take(20).collect()
}

fn json_error(error: serde_json::Error) -> MciError {
    MciError::Json(error.to_string())
}

fn print_usage() {
    println!(
        "\
MCI Internet Packages Client

Usage:
  mci-client [OPTIONS] [COMMAND]

Commands:
  gui       Show the floating desktop label (default)
  unused    Print unused package amounts in MB
  json      Print the compatibility JSON summary
  raw       Print the full packages response
  test      Run the diagnostic flow

Options:
  -e, --env <PATH>          .env file path (default: .env)
  -i, --interval <SECONDS>  GUI polling interval (default: PULL_INTERVAL_SECONDS or 10)
  -h, --help               Show this help
"
    );
}
