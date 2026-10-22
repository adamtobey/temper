//! Versioned, strict contracts and benchmark objectives.

use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};

/// Initial native ABI: an owned UTF-8 request and response, usually JSON.
pub const ABI: &str = "temper.lean.string.v1";

/// Explicit proved program type; legacy packages keep the string boundary.
#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Abi {
    /// UTF-8 request and response.
    #[default]
    StringV1,
    /// UInt64 parameter and binary buffers, returning a typed error or buffers.
    BuffersV1,
}

/// Release whose checker API and native initialization ABI this adapter supports.
pub const TOOLCHAIN: &str = "leanprover/lean4:v4.30.0-rc2";

/// Specification metadata. Lean sources are immutable inputs, not build scripts.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Manifest {
    /// Manifest format, currently 1.
    pub version: u32,
    /// Explicit tenant; never inferred from another installed app.
    pub tenant: String,
    /// Component name within that tenant.
    pub component: String,
    /// Exact Lean toolchain name, including its release version.
    pub toolchain: String,
    /// Exact native function type, checked against the proved entry point.
    #[serde(default)]
    pub abi: Abi,
    /// Required, already-proved declarations in Spec.lean.
    pub specification_proofs: Vec<String>,
    /// Fixed propositions over a program; candidate proofs use the same names.
    pub obligations: Vec<String>,
    /// Explicit benchmark objective; absent only for verification-only use.
    pub objective: Option<Objective>,
}

/// Benchmark-defined optimization objective.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Objective {
    /// Metric observed by the host, never supplied by the candidate.
    pub metric: Metric,
    /// Whether a larger or smaller measurement wins.
    pub direction: Direction,
    /// Requests for the compiled program. Every request is exercised per sample.
    pub inputs: Vec<String>,
    /// Content-pinned files loaded before timing for the binary batch ABI.
    #[serde(default)]
    pub workloads: Vec<BufferWorkload>,
    /// Number of independent samples, aggregated by their median.
    pub samples: u32,
    /// Complete workload repetitions per sample.
    pub iterations: u32,
    /// Minimum fractional improvement relative to the current parent.
    pub minimum_improvement: f64,
    /// Optional pinned host benchmark for memory, cost, or other custom metrics.
    #[serde(default)]
    pub external: Option<ExternalBenchmark>,
    /// Native measurement process budget, independent of generation budgets.
    #[serde(default = "benchmark_seconds")]
    pub seconds: u64,
}

fn benchmark_seconds() -> u64 {
    60
}

/// One invocation against pre-generated files; generating/reading files is untimed.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct BufferWorkload {
    /// Human-readable workload/distribution name.
    pub name: String,
    /// Typed parameter interpreted by the fixed app specification.
    pub parameter: u64,
    /// Input buffers with exact, immutable content identities.
    pub files: Vec<InputFile>,
}

/// A file whose bytes, not a mutable filename alone, define a benchmark input.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct InputFile {
    /// Absolute path to a pre-generated workload file.
    pub path: std::path::PathBuf,
    /// Ordinary SHA-256, checked and snapshotted at the specification gate.
    pub sha256: String,
}

/// Trusted benchmark executable, fixed before candidate generation.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ExternalBenchmark {
    /// Absolute executable path. The verified bytes are snapshotted for search.
    pub program: std::path::PathBuf,
    /// SHA-256 of that executable, as 64 lowercase hexadecimal digits.
    pub sha256: String,
    /// Metric name recorded with the objective, for example peak_memory_bytes.
    pub metric: String,
}

/// Built-in metrics measured from the actual compiled artifact.
#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Metric {
    /// Median elapsed nanoseconds per complete workload through the native ABI.
    LatencyNs,
    /// Process CPU nanoseconds per compaction, excluding untimed workload loading.
    CpuNs,
    /// Size in bytes of the native library, measured after running the workload.
    ArtifactBytes,
    /// Requests per second through the native ABI for the fixed workload.
    ThroughputPerSecond,
    /// Numeric output of a separately pinned host benchmark executable.
    Custom,
}

/// Optimization ordering, independent of the units of a metric.
#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Direction {
    /// Prefer smaller observations.
    Minimize,
    /// Prefer larger observations.
    Maximize,
}

impl Manifest {
    /// Validate names and budgets before generating Lean source or starting work.
    pub fn validate(&self) -> Result<()> {
        ensure!(self.version == 1, "unsupported Lean manifest version");
        ensure!(
            !self.tenant.is_empty() && !self.component.is_empty(),
            "tenant and component are required"
        );
        ensure!(
            self.toolchain == TOOLCHAIN,
            "unsupported Lean toolchain; this native ABI requires {TOOLCHAIN}"
        );
        ensure!(
            !self.specification_proofs.is_empty(),
            "specification proofs are required before generation"
        );
        ensure!(
            !self.obligations.is_empty(),
            "implementation obligations cannot be empty"
        );
        for names in [&self.specification_proofs, &self.obligations] {
            let mut unique = std::collections::BTreeSet::new();
            for name in names {
                ensure!(valid_name(name), "invalid Lean declaration name: {name}");
                ensure!(unique.insert(name), "duplicate Lean obligation: {name}");
            }
        }
        if let Some(objective) = &self.objective {
            ensure!(
                objective.metric != Metric::CpuNs || self.abi == Abi::BuffersV1,
                "CPU-time measurement requires the binary ABI benchmark"
            );
            ensure!(
                match self.abi {
                    Abi::StringV1 =>
                        (1..=1024).contains(&objective.inputs.len())
                            && objective.workloads.is_empty(),
                    Abi::BuffersV1 =>
                        objective.inputs.is_empty()
                            && (1..=1024).contains(&objective.workloads.len()),
                },
                "benchmark inputs must match the selected ABI"
            );
            ensure!(
                (1..=3600).contains(&objective.seconds),
                "invalid benchmark time budget"
            );
            let mut names = std::collections::BTreeSet::new();
            for workload in &objective.workloads {
                ensure!(
                    names.insert(&workload.name),
                    "duplicate binary workload name"
                );
                ensure!(
                    !workload.name.is_empty() && !workload.files.is_empty(),
                    "binary workloads need a name and files"
                );
                for file in &workload.files {
                    ensure!(
                        file.path.is_absolute()
                            && file.sha256.len() == 64
                            && file
                                .sha256
                                .bytes()
                                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)),
                        "binary workload needs absolute paths and SHA-256"
                    );
                }
            }
            ensure!(
                objective.inputs.iter().all(|s| s.len() <= 16 * 1024 * 1024),
                "benchmark input exceeds 16 MiB"
            );
            ensure!(
                (1..=1000).contains(&objective.samples),
                "benchmark samples must be in 1..=1000"
            );
            ensure!(
                (1..=1_000_000).contains(&objective.iterations),
                "benchmark iterations must be in 1..=1000000"
            );
            ensure!(
                objective.minimum_improvement.is_finite()
                    && (0.0..1.0).contains(&objective.minimum_improvement),
                "invalid improvement threshold"
            );
            ensure!(
                (objective.metric == Metric::Custom) == objective.external.is_some(),
                "custom objectives require exactly one pinned external benchmark"
            );
            if let Some(external) = &objective.external {
                ensure!(
                    external.program.is_absolute() && !external.metric.is_empty(),
                    "custom benchmark needs an absolute path and metric name"
                );
                ensure!(
                    external.sha256.len() == 64
                        && external
                            .sha256
                            .bytes()
                            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)),
                    "invalid benchmark SHA-256"
                );
            }
        }
        Ok(())
    }
}

fn valid_name(name: &str) -> bool {
    !name.is_empty()
        && name.split('.').all(|part| {
            !part.is_empty()
                && part.as_bytes()[0].is_ascii_alphabetic()
                && part.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'_')
        })
}
