use std::path::PathBuf;

use anyhow::{Result, bail};
use camino::Utf8PathBuf;
use clap::{Parser, Subcommand};
use snesmaker_export::{build_rom, run_with_emulator};
use snesmaker_project::ProjectBundle;
use snesmaker_validator::{Severity, validate_project};
use tracing_subscriber::EnvFilter;

#[derive(Debug, Parser)]
#[command(name = "snesmaker")]
#[command(about = "Rust-first SNES game maker CLI")]
struct Cli {
    #[command(subcommand)]
    command: CommandKind,
}

#[derive(Debug, Subcommand)]
enum CommandKind {
    New {
        path: PathBuf,
        #[arg(long)]
        name: Option<String>,
    },
    Check {
        #[arg(default_value = ".")]
        path: PathBuf,
    },
    BuildRom {
        #[arg(default_value = ".")]
        path: PathBuf,
        #[arg(long)]
        out: Option<PathBuf>,
    },
    Run {
        #[arg(default_value = ".")]
        path: PathBuf,
        #[arg(long)]
        out: Option<PathBuf>,
        #[arg(long)]
        emulator: Option<String>,
    },
}

fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .without_time()
        .init();

    let cli = Cli::parse();

    match cli.command {
        CommandKind::New { path, name } => {
            let project_root = utf8_path(path)?;
            let project_name = name.unwrap_or_else(|| {
                project_root
                    .file_name()
                    .map(str::to_string)
                    .unwrap_or_else(|| "My SNES Game".to_string())
            });
            ProjectBundle::write_template_project(&project_root, &project_name)?;
            println!("Created project '{}' at {}", project_name, project_root);
        }
        CommandKind::Check { path } => {
            let project_root = utf8_path(path)?;
            let bundle = ProjectBundle::load(&project_root)?;
            let report = validate_project(&bundle);
            print_report(&report);
            if !report.is_ok() {
                bail!("validation failed");
            }
        }
        CommandKind::BuildRom { path, out } => {
            let project_root = utf8_path(path)?;
            let out = out.map(utf8_path).transpose()?;
            let outcome = build_rom(&project_root, out.as_deref())?;
            print_report(&outcome.validation);
            println!("Build report: {}", outcome.report_path);
            if outcome.rom_built {
                println!("ROM: {}", outcome.rom_path);
                if let Some(stable_rom_path) = &outcome.stable_rom_path {
                    println!("Stable ROM alias: {}", stable_rom_path);
                }
            } else {
                println!("ROM not built yet:");
                for warning in outcome.assembler_status.warnings {
                    println!("  - {warning}");
                }
            }
        }
        CommandKind::Run {
            path,
            out,
            emulator,
        } => {
            let project_root = utf8_path(path)?;
            let out = out.map(utf8_path).transpose()?;
            let outcome = run_with_emulator(&project_root, out.as_deref(), emulator.as_deref())?;
            println!(
                "Launched {} with {}",
                outcome.rom_path,
                emulator.unwrap_or_else(|| "configured emulator".to_string())
            );
        }
    }

    Ok(())
}

fn utf8_path(path: PathBuf) -> Result<Utf8PathBuf> {
    Utf8PathBuf::from_path_buf(path).map_err(|_| anyhow::anyhow!("path must be valid UTF-8"))
}

fn print_report(report: &snesmaker_validator::ValidationReport) {
    if report.errors.is_empty() && report.warnings.is_empty() {
        println!("Validation passed with no diagnostics.");
    }

    for diagnostic in report.errors.iter().chain(report.warnings.iter()) {
        let severity = match diagnostic.severity {
            Severity::Error => "error",
            Severity::Warning => "warning",
        };
        let path = diagnostic
            .path
            .as_deref()
            .map(|path| format!(" ({path})"))
            .unwrap_or_default();
        println!(
            "[{severity}] {}: {}{path}",
            diagnostic.code, diagnostic.message
        );
    }

    println!(
        "Budget summary: {} scene(s), {} tile(s), {} palette color(s), ~{} bank(s)",
        report.budgets.scene_count,
        report.budgets.unique_tiles,
        report.budgets.palette_colors,
        report.budgets.estimated_rom_banks
    );
}
