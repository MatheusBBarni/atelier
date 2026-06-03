use crate::config::{init_config, load_effective_config, to_redacted_toml, ConfigLoadOptions};
use crate::doctor::{render_human, render_json, run_doctor};
use crate::history::clean_sessions;
use anyhow::{bail, Context, Result};
use clap::Parser;
use std::env;
use std::io::{self, Write};
use std::path::PathBuf;

#[derive(Clone, Debug, Parser)]
#[command(name = "multiagent")]
#[command(about = "Terminal-native multiagent harness")]
pub struct Cli {
    #[arg(long)]
    pub cwd: Option<PathBuf>,

    #[arg(long)]
    pub config: Option<PathBuf>,

    #[arg(long)]
    pub doctor: bool,

    #[arg(long)]
    pub json: bool,

    #[arg(long)]
    pub print_config: bool,

    #[arg(long)]
    pub init_config: bool,

    #[arg(long)]
    pub clean_sessions: bool,

    #[arg(long)]
    pub yes: bool,

    #[arg(long)]
    pub debug: bool,
}

pub async fn run_cli() -> Result<()> {
    let cli = Cli::parse();
    run_cli_with(cli).await
}

pub async fn run_cli_with(cli: Cli) -> Result<()> {
    if cli.json && !cli.doctor {
        bail!("--json is only valid with --doctor");
    }
    if cli.yes && !cli.clean_sessions {
        bail!("--yes is only valid with --clean-sessions");
    }

    if cli.init_config {
        let path = cli
            .config
            .clone()
            .or_else(|| env::var_os("MULTIAGENT_CONFIG").map(PathBuf::from));
        let summary = init_config(path)?;
        for path in &summary.created {
            println!("created {}", path.display());
        }
        for path in &summary.skipped {
            println!("skipped {}", path.display());
        }
        return Ok(());
    }

    let working_directory = match &cli.cwd {
        Some(path) => path.clone(),
        None => env::current_dir().context("failed to read current working directory")?,
    };
    let config = load_effective_config(ConfigLoadOptions {
        working_directory,
        config_path: cli.config.clone(),
    })?;

    if cli.print_config {
        print!("{}", to_redacted_toml(&config)?);
        return Ok(());
    }

    if cli.doctor {
        let report = run_doctor(&config).await;
        if cli.json {
            println!("{}", render_json(&report)?);
        } else {
            print!("{}", render_human(&report));
        }
        return Ok(());
    }

    if cli.clean_sessions {
        if !cli.yes && !confirm_cleanup(&config.working_directory)? {
            println!("session cleanup cancelled");
            return Ok(());
        }
        let deleted = clean_sessions(&config.working_directory)?;
        if deleted.is_empty() {
            println!(
                "no session history found under {}",
                config.working_directory.display()
            );
        } else {
            for path in deleted {
                println!("deleted {}", path.display());
            }
        }
        return Ok(());
    }

    crate::tui::run_tui(config, cli.debug).await
}

fn confirm_cleanup(working_directory: &std::path::Path) -> Result<bool> {
    eprint!(
        "Delete multiagent session history under {}? [y/N] ",
        working_directory.display()
    );
    io::stderr().flush()?;
    let mut answer = String::new();
    io::stdin().read_line(&mut answer)?;
    Ok(matches!(answer.trim(), "y" | "Y" | "yes" | "YES"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[tokio::test]
    async fn print_config_renders_toml() {
        let dir = tempdir().unwrap();
        let cli = Cli {
            cwd: Some(dir.path().to_path_buf()),
            config: None,
            doctor: false,
            json: false,
            print_config: true,
            init_config: false,
            clean_sessions: false,
            yes: false,
            debug: false,
        };
        run_cli_with(cli).await.unwrap();
    }
}
