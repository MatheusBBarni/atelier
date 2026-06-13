use crate::codemap::{render_summary as render_codemap_summary, run_codemap, CodemapCommand};
use crate::config::{init_config, load_effective_config, to_redacted_toml, ConfigLoadOptions};
use crate::docgen::{emit_docs, render_summary as render_docgen_summary};
use crate::doctor::{render_human, render_json, run_doctor};
use crate::history::clean_sessions;
use anyhow::{bail, Context, Result};
use clap::Parser;
use std::env;
use std::io::{self, Write};
use std::path::PathBuf;

#[derive(Clone, Debug, Parser)]
#[command(
    name = "atelier",
    version = env!("CARGO_PKG_VERSION"),
    about = "Terminal-native agent orchestration harness"
)]
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
    pub update: bool,

    #[arg(long, value_enum)]
    pub codemap: Option<CodemapCommand>,

    #[arg(long, value_name = "DIR")]
    pub emit_docs: Option<PathBuf>,

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
    if cli.update
        && (cli.cwd.is_some()
            || cli.config.is_some()
            || cli.doctor
            || cli.json
            || cli.print_config
            || cli.init_config
            || cli.clean_sessions
            || cli.codemap.is_some()
            || cli.emit_docs.is_some()
            || cli.yes
            || cli.debug)
    {
        bail!("--update cannot be combined with other flags");
    }
    if cli.codemap.is_some()
        && (cli.doctor
            || cli.print_config
            || cli.init_config
            || cli.clean_sessions
            || cli.emit_docs.is_some())
    {
        bail!(
            "--codemap cannot be combined with --doctor, --print-config, --init-config, --clean-sessions, or --emit-docs"
        );
    }
    if cli.emit_docs.is_some()
        && (cli.doctor || cli.print_config || cli.init_config || cli.clean_sessions)
    {
        bail!(
            "--emit-docs cannot be combined with --doctor, --print-config, --init-config, or --clean-sessions"
        );
    }

    if cli.update {
        bail!(
            "atelier --update is handled by the npm launcher. Run `npm install -g @matheusbbarni/atelier@latest --include=optional` to update this native binary manually."
        );
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
        println!(
            "review {} before running agents",
            summary.config_path.display()
        );
        println!("then run atelier --doctor to check runtime setup");
        return Ok(());
    }

    let working_directory = match &cli.cwd {
        Some(path) => path.clone(),
        None => env::current_dir().context("failed to read current working directory")?,
    };

    if let Some(command) = cli.codemap {
        let summary = run_codemap(&working_directory, command)?;
        print!("{}", render_codemap_summary(&summary));
        return Ok(());
    }

    let config = load_effective_config(ConfigLoadOptions {
        working_directory,
        config_path: cli.config.clone(),
    })?;

    if cli.print_config {
        print!("{}", to_redacted_toml(&config)?);
        return Ok(());
    }

    if let Some(out_dir) = &cli.emit_docs {
        let summary = emit_docs(&config, out_dir)?;
        print!("{}", render_docgen_summary(&summary));
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
        "Delete atelier session history under {}? [y/N] ",
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
    use clap::CommandFactory;
    use tempfile::tempdir;

    #[test]
    fn clap_command_name_is_atelier() {
        assert_eq!(Cli::command().get_name(), "atelier");
    }

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
            update: false,
            codemap: None,
            emit_docs: None,
            yes: false,
            debug: false,
        };
        run_cli_with(cli).await.unwrap();
    }

    #[tokio::test]
    async fn codemap_init_writes_state_without_loading_runtime_config() {
        let dir = tempdir().unwrap();
        std::fs::write(dir.path().join("main.rs"), "fn main() {}").unwrap();
        let cli = Cli {
            cwd: Some(dir.path().to_path_buf()),
            config: None,
            doctor: false,
            json: false,
            print_config: false,
            init_config: false,
            clean_sessions: false,
            update: false,
            codemap: Some(CodemapCommand::Init),
            emit_docs: None,
            yes: false,
            debug: false,
        };

        run_cli_with(cli).await.unwrap();

        assert!(dir.path().join(".multiagent/codemap.json").exists());
        assert!(dir.path().join("codemap.md").exists());
    }

    fn cli_with_emit_docs(cwd: PathBuf, out_dir: PathBuf) -> Cli {
        Cli {
            cwd: Some(cwd),
            config: None,
            doctor: false,
            json: false,
            print_config: false,
            init_config: false,
            clean_sessions: false,
            update: false,
            codemap: None,
            emit_docs: Some(out_dir),
            yes: false,
            debug: false,
        }
    }

    #[tokio::test]
    async fn emit_docs_writes_reference_fragments() {
        let dir = tempdir().unwrap();
        let out_dir = dir.path().join("_generated");
        let cli = cli_with_emit_docs(dir.path().to_path_buf(), out_dir.clone());

        run_cli_with(cli).await.unwrap();

        assert!(out_dir.join("configuration.md").exists());
        assert!(out_dir.join("cli.md").exists());
    }

    #[tokio::test]
    async fn emit_docs_conflicts_with_print_config() {
        let dir = tempdir().unwrap();
        let mut cli = cli_with_emit_docs(dir.path().to_path_buf(), dir.path().join("_generated"));
        cli.print_config = true;

        let error = run_cli_with(cli).await.unwrap_err();
        assert!(error.to_string().contains("--emit-docs cannot be combined"));
    }

    #[tokio::test]
    async fn emit_docs_conflicts_with_update() {
        let dir = tempdir().unwrap();
        let mut cli = cli_with_emit_docs(dir.path().to_path_buf(), dir.path().join("_generated"));
        cli.update = true;

        let error = run_cli_with(cli).await.unwrap_err();
        assert!(error.to_string().contains("--update cannot be combined"));
    }
}
