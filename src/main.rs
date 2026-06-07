#![cfg_attr(windows, windows_subsystem = "windows")]

mod gui;

use std::env;
use std::path::{Path, PathBuf};
use std::process;

use jahan_nama::format::{error_json, remain_json, remaining_label};
use jahan_nama::{DotEnvStore, JahanNamaClient, JahanNamaError, Result};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Command {
    Gui,
    Json,
    Raw,
    Remain,
    Test,
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
            let mut client = JahanNamaClient::new(&args.env_path)?;
            let payload = client.get_remain_response()?;
            println!(
                "{}",
                serde_json::to_string_pretty(&payload).map_err(json_error)?
            );
            Ok(())
        }
        Command::Test => run_test(&args.env_path),
        Command::Remain => {
            let mut client = JahanNamaClient::new(&args.env_path)?;
            let megabytes = client.get_remaining_traffic_mb()?;
            println!("{}", remaining_label(megabytes));
            Ok(())
        }
        Command::Json | Command::Help => Ok(()),
    }
}

fn run_json(env_path: &Path) -> Result<()> {
    let mut client = JahanNamaClient::new(env_path)?;
    let megabytes = client.get_remaining_traffic_mb()?;
    println!("{}", remain_json(megabytes));
    Ok(())
}

fn run_test(env_path: &Path) -> Result<()> {
    println!("=== Jahan Nama Client Test ===");

    let mut client = JahanNamaClient::new(env_path)?;
    println!("\n[1] Checking existing token status...");
    println!("Token exists: {}", client.token().is_some());

    println!("\n[2] Ensuring valid token...");
    let token = client.ensure_token(false)?;
    println!("Token acquired: {}...", token_preview(&token));

    println!("\n[3] Fetching remaining traffic...");
    let payload = client.get_remain_response()?;
    println!("Remaining traffic fetched successfully.");

    println!("\n--- Raw Response (truncated) ---");
    let raw = serde_json::to_string_pretty(&payload).map_err(json_error)?;
    println!("{}...", raw.chars().take(300).collect::<String>());

    println!("\n[4] Extracting RemainTraffic...");
    let megabytes = jahan_nama::remain_traffic_mb(&payload).ok_or(
        JahanNamaError::UnexpectedResponse("Unexpected remaining traffic response format."),
    )?;
    println!(
        "Remaining traffic: {megabytes:.2} MB ({})",
        remaining_label(megabytes)
    );

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
            "remain" | "unused" => set_command(&mut command, Command::Remain)?,
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
                env.get("JAHAN_NAMA_INTERVAL_SECONDS")
                    .and_then(|value| value.parse().ok())
            })
        })
        .filter(|value| *value > 0)
        .unwrap_or(10)
}

fn token_preview(token: &str) -> String {
    token.chars().take(20).collect()
}

fn json_error(error: serde_json::Error) -> JahanNamaError {
    JahanNamaError::Json(error.to_string())
}

fn print_usage() {
    println!(
        "\
Jahan Nama Remaining Traffic Client

Usage:
  jahan-nama [OPTIONS] [COMMAND]

Commands:
  gui       Show the floating desktop label (default)
  remain    Print remaining traffic
  unused    Alias for remain
  json      Print the JSON summary
  raw       Print the full remaining traffic response
  test      Run the diagnostic flow

Options:
  -e, --env <PATH>          .env file path (default: .env)
  -i, --interval <SECONDS>  GUI polling interval (default: JAHAN_NAMA_INTERVAL_SECONDS or 10)
  -h, --help               Show this help
"
    );
}
