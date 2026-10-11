//! `orcinus` — the Orcinus agent CLI (spec §3.6). This skeleton ships
//! `status`, `open` and `skills get`; `computer *` lands in the next slice.
//!
//! Output contract: with `--json`, success prints
//! `{"id":<uuid>,"ok":true,"result":…,"_meta":{"runtimeId":…}}` and failure
//! prints `{"id":"local","ok":false,"error":{code,message,data?}}` then exits
//! 1.
//!
//! Human mode prints readable text and exits 1 on any error (and for `status`
//! when the runtime is unreachable).

mod client;
mod errors;
mod open;
mod skills;
mod status;

use clap::{Parser, Subcommand};

use errors::{success_envelope, CliError};

#[derive(Debug, Parser)]
#[command(
    name = "orcinus",
    version,
    about = "Drive Orcinus and its computer-use runtime from the terminal",
    disable_help_subcommand = true
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Report whether the Orcinus runtime is running (never launches it).
    Status {
        /// Emit the JSON envelope instead of readable text.
        #[arg(long)]
        json: bool,
    },
    /// Launch Orcinus and wait for its runtime to become reachable.
    Open {
        /// Emit the JSON envelope instead of readable text.
        #[arg(long)]
        json: bool,
    },
    /// Work with bundled agent-guide skills.
    Skills {
        #[command(subcommand)]
        command: SkillsCommand,
    },
}

#[derive(Debug, Subcommand)]
enum SkillsCommand {
    /// Print a bundled skill guide.
    Get {
        /// Guide topic; the only bundled topic today is `computer-use`.
        topic: String,
        /// Accepted for oracle parity; the embedded guide has no separate
        /// full/reference split, so the body is identical either way.
        #[arg(long)]
        full: bool,
        /// Emit the JSON envelope instead of readable markdown.
        #[arg(long)]
        json: bool,
    },
}

fn main() {
    let cli = Cli::parse();
    let json_output = match &cli.command {
        Command::Status { json } | Command::Open { json } => *json,
        Command::Skills {
            command: SkillsCommand::Get { json, .. },
        } => *json,
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
        Command::Skills { command } => run_skills(command),
    }
}

fn run_status(json_output: bool) -> Result<(), CliError> {
    let (snapshot, runtime_id) = status::read_status();
    let envelope_runtime_id = if runtime_id.is_empty() {
        "none"
    } else {
        runtime_id.as_str()
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
        return Ok(());
    }

    println!("{}", snapshot.to_human());
    if !snapshot.reachable {
        // Why: human-mode exit 1 mirrors the oracle; --json always exits 0 so
        // scripts can read `runtime.reachable` from the envelope.
        std::process::exit(1);
    }
    Ok(())
}

fn run_open(json_output: bool) -> Result<(), CliError> {
    let (snapshot, runtime_id) = open::open()?;
    let envelope_runtime_id = if runtime_id.is_empty() {
        "none"
    } else {
        runtime_id.as_str()
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

fn run_skills(command: SkillsCommand) -> Result<(), CliError> {
    match command {
        SkillsCommand::Get {
            topic,
            full: _full,
            json,
        } => {
            let guide = skills::get(&topic)?;
            if json {
                let envelope =
                    success_envelope(&uuid::Uuid::new_v4().to_string(), "none", guide.json());
                println!("{envelope}");
            } else {
                println!("{}", guide.markdown);
            }
            Ok(())
        }
    }
}

fn report_error(error: &CliError, json_output: bool) {
    if json_output {
        println!("{}", error.envelope());
    } else {
        eprintln!("{}", error.message);
    }
}
