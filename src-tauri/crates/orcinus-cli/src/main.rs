//! `orcinus` — the Orcinus agent CLI (spec §3.6): `status` and `open`. The
//! runtime RPC was removed with the computer-use trim, so neither command
//! observes a live runtime: `status` reports the local runtime metadata file
//! state and `open` launches the app without waiting.
//!
//! Output contract: with `--json`, success prints
//! `{"id":<uuid>,"ok":true,"result":…,"_meta":{"runtimeId":…}}` and failure
//! prints `{"id":"local","ok":false,"error":{code,message,data?}}` then exits
//! 1.
//!
//! Human mode prints readable text; errors print `error <code>: <message>`
//! and exit 1.

mod client;
mod errors;
mod open;
mod status;

use clap::{Parser, Subcommand};
use serde_json::json;

use errors::{success_envelope, CliError};

#[derive(Debug, Parser)]
#[command(
    name = "orcinus",
    version,
    about = "Launch Orcinus and report its local runtime metadata",
    disable_help_subcommand = true
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Report local Orcinus runtime metadata state (never launches it).
    Status {
        /// Emit the JSON envelope instead of readable text.
        #[arg(long)]
        json: bool,
    },
    /// Launch Orcinus (does not wait for a runtime).
    Open {
        /// Emit the JSON envelope instead of readable text.
        #[arg(long)]
        json: bool,
    },
}

fn main() {
    let cli = Cli::parse();
    let json_output = match &cli.command {
        Command::Status { json } | Command::Open { json } => *json,
    };
    match run(cli) {
        Ok(()) => {}
        Err(error) => {
            report_error(&error, json_output);
            std::process::exit(1);
        }
    }
}

fn run(cli: Cli) -> Result<(), CliError> {
    match cli.command {
        Command::Status { json } => run_status(json),
        Command::Open { json } => run_open(json),
    }
}

fn run_status(json_output: bool) -> Result<(), CliError> {
    let snapshot = status::read_status();
    let envelope_runtime_id = if snapshot.runtime_id.is_empty() {
        "none"
    } else {
        snapshot.runtime_id.as_str()
    };

    if json_output {
        println!(
            "{}",
            success_envelope(
                &uuid::Uuid::new_v4().to_string(),
                envelope_runtime_id,
                snapshot.to_result()
            )
        );
    } else {
        println!("{}", snapshot.to_human());
    }
    Ok(())
}

fn run_open(json_output: bool) -> Result<(), CliError> {
    open::open()?;

    if json_output {
        println!(
            "{}",
            success_envelope(
                &uuid::Uuid::new_v4().to_string(),
                "none",
                json!({ "launched": true })
            )
        );
    } else {
        println!("Orcinus launched.");
    }
    Ok(())
}

fn report_error(error: &CliError, json_output: bool) {
    if json_output {
        println!("{}", error.envelope());
    } else {
        eprintln!("error {}: {}", error.code, error.message);
    }
}
