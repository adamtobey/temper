//! CLI surface shared by `temper lean` and the standalone `temper-lean` binary.

use crate::{
    artifact,
    manifest::Manifest,
    search::{Attempt, Episode, Proposer},
    toolchain::{Toolchain, VerifiedCandidate, VerifiedSpec},
};
use anyhow::{Context, Result, ensure};
use clap::{Parser, Subcommand};
use std::{
    path::{Path, PathBuf},
    process::Command,
};

/// Lean verification, search, and reviewed deployment commands.
#[derive(Debug, Subcommand)]
pub enum LeanCommand {
    /// Check all specification proofs before allowing implementation generation.
    Verify {
        /// Directory containing manifest.json and Spec.lean.
        package: PathBuf,
    },
    /// Check every implementation obligation and build its exact native artifact.
    Check {
        /// Verified specification package.
        package: PathBuf,
        /// Candidate.lean source, importing Spec.
        candidate: PathBuf,
        /// New directory in which to retain the artifact and proof receipt.
        #[arg(long)]
        output: PathBuf,
    },
    /// Iterate verified, benchmarked generations without deployment review.
    Optimize {
        /// Immutable specification and benchmark package.
        package: PathBuf,
        /// Sources evaluated in order; accepted winners become subsequent parents.
        #[arg(long)]
        candidate: Vec<PathBuf>,
        /// External executable; receives a JSON context filename as its argument
        /// and emits candidate Lean source on stdout. No model-specific code runs
        /// in the kernel. Invocations have a 120-second budget.
        #[arg(long, conflicts_with = "candidate")]
        proposer: Option<PathBuf>,
        /// Maximum proposed candidates, including failed proofs.
        #[arg(long, default_value_t = 10)]
        budget: u32,
        /// New output directory for the winner and complete search history.
        #[arg(long)]
        output: PathBuf,
    },
    /// Explicitly review one exact artifact for deployment.
    Review {
        /// Artifact directory produced by check or optimize.
        artifact: PathBuf,
        /// Candidate ID from verification/search output, not a floating app name.
        #[arg(long)]
        expected_id: String,
        /// Authenticated local review identity supplied by the operator.
        #[arg(long)]
        reviewer: String,
    },
    /// Invoke a reviewed native core from Rust.
    Invoke {
        /// Reviewed artifact directory.
        artifact: PathBuf,
        /// Explicit tenant owning the artifact.
        #[arg(long)]
        tenant: String,
        /// UTF-8 input, conventionally a JSON state/action envelope.
        #[arg(long)]
        input: String,
    },
}

/// Standalone entry point useful when only Lean kernel tooling is needed.
#[derive(Parser)]
#[command(name = "temper-lean")]
pub struct LeanCli {
    /// Lean operation.
    #[command(subcommand)]
    pub command: LeanCommand,
}

/// Execute a Lean operation. Failures are errors, never successful empty receipts.
pub fn run(command: LeanCommand) -> Result<()> {
    match command {
        LeanCommand::Verify { package } => {
            let spec = specification(&package)?;
            println!(
                "{}",
                serde_json::json!({"specification": spec.id(), "verified": true})
            );
        }
        LeanCommand::Check {
            package,
            candidate,
            output,
        } => {
            let spec = specification(&package)?;
            let candidate = Toolchain.check(&spec, std::fs::read_to_string(candidate)?)?;
            artifact::save(&candidate, &output)?;
            println!(
                "{}",
                serde_json::json!({"specification": spec.id(), "candidate": candidate.id(), "deployment_review_required": true})
            );
        }
        LeanCommand::Optimize {
            package,
            candidate,
            proposer,
            budget,
            output,
        } => {
            ensure!(!output.exists(), "output directory already exists");
            let spec = specification(&package)?;
            let mut episode = Episode::new(&spec, budget)?;
            if let Some(program) = proposer {
                let mut proposer = CommandProposer {
                    program: program.canonicalize()?,
                };
                episode.run(&mut proposer)?;
            } else {
                ensure!(
                    !candidate.is_empty(),
                    "supply candidate sources or an external proposer"
                );
                for path in candidate.into_iter().take(budget as usize) {
                    episode.submit(std::fs::read_to_string(path)?)?;
                }
            }
            if let Some(best) = episode.best() {
                artifact::save(best, &output)?;
            } else {
                std::fs::create_dir(&output)?;
            }
            let history = serde_json::to_vec_pretty(episode.history())?;
            std::fs::write(output.join("history.json"), history)?;
            let best = episode.best().context("no candidate passed proof checking and benchmarks; diagnostics retained in history.json")?;
            println!(
                "{}",
                serde_json::json!({"specification": spec.id(), "candidate": best.id(), "history": episode.history(), "deployment_review_required": true})
            );
        }
        LeanCommand::Review {
            artifact: directory,
            expected_id,
            reviewer,
        } => {
            artifact::review(&directory, &expected_id, &reviewer)?;
            println!(
                "{}",
                serde_json::json!({"candidate": expected_id, "reviewer": reviewer})
            );
        }
        LeanCommand::Invoke {
            artifact: directory,
            tenant,
            input,
        } => {
            let core = artifact::load_reviewed(&directory)?.load()?;
            println!("{}", core.invoke(&tenant, &input)?);
        }
    }
    Ok(())
}

fn specification(directory: &Path) -> Result<VerifiedSpec> {
    let manifest: Manifest =
        serde_json::from_slice(&std::fs::read(directory.join("manifest.json"))?)?;
    Toolchain.verify_spec(
        manifest,
        std::fs::read_to_string(directory.join("Spec.lean"))?,
    )
}

struct CommandProposer {
    program: PathBuf,
}

impl Proposer for CommandProposer {
    fn propose(
        &mut self,
        spec: &VerifiedSpec,
        parent: Option<&VerifiedCandidate>,
        history: &[Attempt],
    ) -> Result<Option<String>> {
        let context = tempfile::tempdir()?;
        let file = context.path().join("context.json");
        std::fs::write(
            &file,
            serde_json::to_vec(&serde_json::json!({
                "specification": spec.id(), "source": spec.source(), "manifest": spec.manifest(),
                "parent": parent.map(|p| serde_json::json!({"id": p.id(), "source": p.source()})), "history": history,
            }))?,
        )?;
        let mut command = Command::new(&self.program);
        command.arg(file);
        let source = crate::process::run(command, context.path(), 120)?;
        Ok(if source.trim().is_empty() {
            None
        } else {
            Some(source)
        })
    }
}
