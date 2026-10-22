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
    /// Prepare the pinned lean-ffi adapter (only this explicit setup may fetch dependencies).
    SetupFfi {
        /// Require already cached Rust dependencies.
        #[arg(long)]
        offline: bool,
    },
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
        /// in the kernel. Invocations use --proposer-seconds (default 900).
        #[arg(long, conflicts_with = "candidate")]
        proposer: Option<PathBuf>,
        /// Verified starting implementations, before iterative generation.
        #[arg(long, requires = "proposer")]
        seed: Vec<PathBuf>,
        /// Time budget per generator call, including model inference and proof repair.
        #[arg(long, default_value_t = 900, value_parser = clap::value_parser!(u64).range(1..=3600))]
        proposer_seconds: u64,
        /// Maximum proposed candidates, including failed proofs.
        #[arg(long, default_value_t = 32)]
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
    /// Invoke a reviewed typed core with binary files and write its output buffers.
    InvokeBuffers {
        /// Reviewed binary-ABI artifact directory.
        artifact: PathBuf,
        /// Explicit owning tenant.
        #[arg(long)]
        tenant: String,
        /// Typed UInt64 argument interpreted by the component specification.
        #[arg(long)]
        parameter: u64,
        /// Pre-existing input files, one per buffer; their contents are not JSON/hex.
        #[arg(long)]
        input: Vec<PathBuf>,
        /// New output directory; never overwrite existing app data.
        #[arg(long)]
        output: PathBuf,
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
        LeanCommand::SetupFfi { offline } => {
            Toolchain.setup_ffi(offline)?;
            println!("{}", serde_json::json!({"ffi_ready": true}));
        }
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
            seed,
            proposer_seconds,
            budget,
            output,
        } => {
            ensure!(!output.exists(), "output directory already exists");
            let spec = specification(&package)?;
            let mut episode = Episode::new(&spec, budget)?;
            let iterative = proposer.is_some();
            let seed_count = seed.len();
            let checkpoints = PathBuf::from(format!("{}.search", output.display()));
            ensure!(
                !checkpoints.exists(),
                "search checkpoint directory already exists"
            );
            std::fs::create_dir(&checkpoints)?;
            let checkpoint =
                |best: Option<&VerifiedCandidate>, history: &[Attempt]| -> Result<()> {
                    use std::io::Write;
                    if let Some(best) = best {
                        let winner = checkpoints.join(best.id());
                        if !winner.exists() {
                            artifact::save(best, &winner)?;
                        }
                    }
                    if let Some(last) = history.last() {
                        if let Some(source) = &last.source {
                            std::fs::write(
                                checkpoints.join(format!("{:04}.lean", history.len())),
                                source,
                            )?;
                        }
                        eprintln!(
                            "Lean search {}/{}: {} {}",
                            history.len(),
                            budget,
                            if last.accepted {
                                "improvement"
                            } else {
                                "retained parent"
                            },
                            last.measurement
                                .map(|m| m.to_string())
                                .or_else(|| last.diagnostic.clone())
                                .unwrap_or_default()
                        );
                    }
                    let mut temporary = tempfile::NamedTempFile::new_in(&checkpoints)?;
                    temporary.write_all(&serde_json::to_vec_pretty(history)?)?;
                    temporary.persist(checkpoints.join("history.json"))?;
                    Ok(())
                };
            for path in seed {
                episode.submit(std::fs::read_to_string(path)?)?;
                checkpoint(episode.best(), episode.history())?;
            }
            if let Some(program) = proposer {
                let mut proposer = CommandProposer {
                    program: program.canonicalize()?,
                    seconds: proposer_seconds,
                };
                episode.run_observed(&mut proposer, checkpoint)?;
            } else {
                ensure!(
                    !candidate.is_empty(),
                    "supply candidate sources or an external proposer"
                );
                for path in candidate.into_iter().take(budget as usize) {
                    episode.submit(std::fs::read_to_string(path)?)?;
                    checkpoint(episode.best(), episode.history())?;
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
            let measured_generations = episode
                .history()
                .iter()
                .skip(seed_count)
                .filter(|a| a.candidate.is_some())
                .count();
            let improvements = episode
                .history()
                .iter()
                .skip(seed_count)
                .filter(|a| a.accepted && a.parent.is_some())
                .count();
            ensure!(
                !iterative || measured_generations > 0,
                "no generated candidate passed proofs and benchmarks; the seed and diagnostics are retained, but this campaign is inconclusive"
            );
            println!(
                "{}",
                serde_json::json!({"specification": spec.id(), "candidate": best.id(), "history": summaries(episode.history()), "history_file":output.join("history.json"), "checkpoints":checkpoints, "attempts":episode.history().len(), "measured_generations":measured_generations, "search_improvements":improvements, "stop_reason":"budget_exhausted_or_supplied_candidates_exhausted", "deployment_review_required": true})
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
        LeanCommand::InvokeBuffers {
            artifact: directory,
            tenant,
            parameter,
            input,
            output,
        } => {
            ensure!(!output.exists(), "output directory already exists");
            let core = artifact::load_reviewed(&directory)?.load()?;
            let files = input
                .iter()
                .map(std::fs::read)
                .collect::<std::io::Result<Vec<_>>>()?;
            let views: Vec<_> = files.iter().map(Vec::as_slice).collect();
            let count =
                core.with_buffers(&tenant, parameter, &views, |result| -> Result<usize> {
                    let outputs =
                        result.map_err(|error| anyhow::anyhow!("Lean component error: {error}"))?;
                    std::fs::create_dir(&output)?;
                    for (index, bytes) in outputs.iter().enumerate() {
                        std::fs::write(output.join(format!("{index:04}.bin")), bytes)?;
                    }
                    Ok(outputs.len())
                })??;
            println!(
                "{}",
                serde_json::json!({"candidate": core.id(), "outputs": count, "directory": output})
            );
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
    seconds: u64,
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
                "generation_seconds": self.seconds,
                "parent": parent.map(|p| serde_json::json!({"id": p.id(), "source": p.source()})),
                "history": summaries(history),
                "recent_proposals": history.iter().rev().filter(|a| a.source.is_some()).take(3).collect::<Vec<_>>(),
                "instruction": "Return complete Lean source with all fixed proofs. Repair recent failed proofs or propose a substantially different optimization. Empty output does not end the campaign. Never edit the contract or benchmark.",
            }))?,
        )?;
        let mut command = Command::new(&self.program);
        command.arg(file);
        let source = crate::process::run(command, context.path(), self.seconds)?;
        Ok(if source.trim().is_empty() {
            None
        } else {
            Some(source)
        })
    }
}

fn summaries(history: &[Attempt]) -> Vec<serde_json::Value> {
    history.iter().map(|attempt| serde_json::json!({
        "parent": attempt.parent, "candidate": attempt.candidate,
        "measurement": attempt.measurement, "measurements": attempt.measurements,
        "reference_measurement": attempt.reference_measurement,
        "accepted": attempt.accepted, "diagnostic": attempt.diagnostic,
        "proposal_header": attempt.source.as_deref().and_then(|s| s.lines().next()).map(|s| s.chars().take(256).collect::<String>()),
    })).collect()
}
