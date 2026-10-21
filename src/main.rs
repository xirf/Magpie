mod agent;
mod config;
mod discord;
mod graph;
mod provider;
mod store;
#[cfg(test)]
#[allow(dead_code)]
mod telegram;
#[cfg(test)]
mod tests;
mod tools;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use std::{
    io::{self, Write},
    path::PathBuf,
};

#[derive(Parser)]
#[command(
    version,
    about = "Mornye: a small persistent assistant with configurable tool approval"
)]
struct Cli {
    #[arg(long, default_value = "mornye.toml")]
    config: PathBuf,
    #[command(subcommand)]
    command: Option<Command>,
}
#[derive(Subcommand)]
enum Command {
    /// Interactive terminal chat. /help lists control commands.
    Chat,
    /// Process one message or slash command, preserving the CLI conversation.
    Ask { message: String },
    /// Run the owner-authorized Discord service and persisted one-time schedules.
    Serve,
    /// Print configuration and runtime status without calling a model.
    Doctor,
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    let config = config::Config::load(&cli.config)?;
    let lock = std::fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(config.state_dir.join("runtime.lock"))?;
    fs2::FileExt::try_lock_exclusive(&lock).context("Another Mornye process owns this state directory. Stop it before opening another instance.")?;
    let agent = agent::Agent::new(config)?;
    match cli.command.unwrap_or(Command::Chat) {
        Command::Doctor => {
            println!(
                "Mornye {}\nArchitecture: {}\nOS: {}\nWorkspace: {}\nState: {}\nProvider: {}\nModel: {}\nAPI key configured: {}\nDiscord owner configured: {}\nExecution: direct host, no sandbox\nApproval: {}",
                env!("CARGO_PKG_VERSION"),
                std::env::consts::ARCH,
                std::env::consts::OS,
                agent.config.workspace.display(),
                agent.config.state_dir.display(),
                agent.config.api_base,
                agent.config.model,
                std::env::var(&agent.config.api_key_env).is_ok_and(|s| !s.is_empty()),
                agent.config.discord_user_id.is_some(),
                agent.config.approval_mode()
            );
        }
        Command::Ask { message } => println!("{}", display(&agent.input("cli", &message).await?)),
        Command::Serve => {
            tokio::select! {
                result = discord::serve(&agent) => result?,
                _ = tokio::signal::ctrl_c() => eprintln!("Stopping. Inspect in-flight actions on restart."),
            }
        }
        Command::Chat => {
            println!(
                "Mornye — /help for commands, /quit to exit. Approval: {}",
                agent.config.approval_mode()
            );
            loop {
                print!("you> ");
                io::stdout().flush()?;
                let mut input = String::new();
                if io::stdin().read_line(&mut input)? == 0 || input.trim() == "/quit" {
                    break;
                }
                let result = tokio::select! {
                    result = agent.input("cli",&input) => result,
                    _ = tokio::signal::ctrl_c() => { eprintln!("Interrupted. Restart and inspect /status before retrying."); break; }
                };
                match result {
                    Ok(reply) => println!("mornye> {}\n", display(&reply)),
                    Err(error) => eprintln!(
                        "Error: {}\nProgress saved. /retry resumes; /cancel stops the turn.",
                        display(&error.to_string())
                    ),
                }
            }
        }
    }
    Ok(())
}
fn display(text: &str) -> String {
    text.chars()
        .filter(|c| !c.is_control() || matches!(c, '\n' | '\t'))
        .collect()
}
