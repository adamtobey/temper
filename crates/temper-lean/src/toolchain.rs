//! Real Lean checking and native compilation, outside actor dispatch.

use crate::{
    identity,
    manifest::{ABI, Abi, Manifest},
    process,
};
use anyhow::{Context, Result, ensure};
use std::{
    path::{Path, PathBuf},
    process::Command,
    sync::Arc,
};

/// Complete specification, checked before any candidate is accepted.
#[derive(Clone)]
pub struct VerifiedSpec {
    pub(crate) manifest: Manifest,
    pub(crate) source: String,
    pub(crate) id: String,
    root: PathBuf,
    version: String,
    contract: Arc<std::collections::BTreeMap<String, Vec<u8>>>,
    pub(crate) external_benchmark: Option<Vec<u8>>,
    pub(crate) ffi_library: Option<Arc<Vec<u8>>>,
    pub(crate) workloads: Arc<Vec<PreparedWorkload>>,
}

impl std::fmt::Debug for VerifiedSpec {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("VerifiedSpec")
            .field("id", &self.id)
            .field("tenant", &self.manifest.tenant)
            .field("component", &self.manifest.component)
            .field("version", &self.version)
            .field("workloads", &self.workloads.len())
            .finish_non_exhaustive()
    }
}

#[derive(Clone, Debug)]
pub(crate) struct PreparedWorkload {
    pub parameter: u64,
    pub files: Vec<Vec<u8>>,
}

impl VerifiedSpec {
    /// Stable identity of all immutable contract inputs.
    pub fn id(&self) -> &str {
        &self.id
    }
    /// Tenant and component metadata bound to this specification.
    pub fn manifest(&self) -> &Manifest {
        &self.manifest
    }
    /// Fixed specification supplied to an external implementation generator.
    pub fn source(&self) -> &str {
        &self.source
    }
    pub(crate) fn benchmark_command(&self, path: &Path, executable: &Path) -> Result<Command> {
        process::command(
            &self.root,
            path,
            executable.to_str().context("benchmark path")?,
            &["workload.bin"],
            true,
        )
    }
}

/// A checked executable. Construction is private to the real proof checker.
#[derive(Clone)]
pub struct VerifiedCandidate {
    pub(crate) spec: VerifiedSpec,
    pub(crate) source: String,
    pub(crate) id: String,
    pub(crate) artifact_hash: String,
    pub(crate) library: PathBuf,
    pub(crate) benchmark: PathBuf,
    pub(crate) measurements: std::collections::BTreeMap<String, f64>,
    pub(crate) _workspace: Arc<tempfile::TempDir>,
}

impl VerifiedCandidate {
    /// Identity of the specification, implementation, and compiled library.
    pub fn id(&self) -> &str {
        &self.id
    }
    /// Specification this implementation satisfies.
    pub fn specification(&self) -> &VerifiedSpec {
        &self.spec
    }
    /// Candidate source for subsequent search generations.
    pub fn source(&self) -> &str {
        &self.source
    }
    /// Actual native artifact content identity.
    pub fn artifact_hash(&self) -> &str {
        &self.artifact_hash
    }
}

/// External checker configuration with finite execution budgets.
pub struct Toolchain;

const CHECKER: &str = include_str!("../lean/Check.lean");
const SOURCE_POLICY: &str = include_str!("source.rs");
const BRIDGE: &str = include_str!("../lean/bridge.c");
const BENCHMARK: &str = include_str!("../lean/benchmark.c");
const ENTRY: &str = "import Candidate\n@[export temper_lean_invoke]\ndef temperEntry (input : String) : String := Candidate.program input\n";
const BUFFER_ENTRY: &str = "import Candidate\n@[export temper_lean_invoke]\ndef temperEntry (parameter : UInt64) (files : Array ByteArray) : Except String (Array ByteArray) := Candidate.program parameter files\n";
const BUFFER_BRIDGE: &str = include_str!("../lean/bridge-buffers.c");
const BUFFER_BENCHMARK: &str = include_str!("../lean/benchmark-buffers.c");

impl Toolchain {
    /// Prepare the trusted pinned Rust bridge; network access is explicit setup only.
    pub fn setup_ffi(&self, offline: bool) -> Result<()> {
        let job = tempfile::tempdir()?;
        let mut command = Command::new("elan");
        command
            .args(["which", "lean"])
            .env("ELAN_TOOLCHAIN", crate::manifest::TOOLCHAIN);
        let path = PathBuf::from(process::run(command, job.path(), 30)?.trim());
        crate::ffi_build::build(
            path.parent().and_then(Path::parent).context("Lean root")?,
            offline,
        )?;
        Ok(())
    }
    /// Check the complete specification before allowing implementation generation.
    pub fn verify_spec(&self, manifest: Manifest, source: String) -> Result<VerifiedSpec> {
        manifest.validate()?;
        crate::source::validate(&source)?;
        let mut workloads = Vec::new();
        if let Some(objective) = &manifest.objective {
            use sha2::{Digest, Sha256};
            for workload in &objective.workloads {
                let mut files = Vec::new();
                for file in &workload.files {
                    let bytes = std::fs::read(&file.path)
                        .with_context(|| format!("read pinned workload {}", file.path.display()))?;
                    ensure!(
                        format!("{:x}", Sha256::digest(&bytes)) == file.sha256,
                        "benchmark input changed: {}",
                        file.path.display()
                    );
                    files.push(bytes);
                }
                workloads.push(PreparedWorkload {
                    parameter: workload.parameter,
                    files,
                });
            }
        }
        let external_benchmark = manifest
            .objective
            .as_ref()
            .and_then(|o| o.external.as_ref())
            .map(|benchmark| -> Result<Vec<u8>> {
                use sha2::{Digest, Sha256};
                let bytes = std::fs::read(&benchmark.program)?;
                ensure!(
                    format!("{:x}", Sha256::digest(&bytes)) == benchmark.sha256,
                    "benchmark executable does not match its pinned hash"
                );
                Ok(bytes)
            })
            .transpose()?;
        let job = tempfile::tempdir()?;
        let mut elan = Command::new("elan");
        elan.args(["which", "lean"])
            .env("ELAN_TOOLCHAIN", &manifest.toolchain);
        let executable = process::run(elan, job.path(), 30)?;
        let root = PathBuf::from(executable.trim())
            .parent()
            .context("missing Lean bin")?
            .parent()
            .context("missing Lean root")?
            .to_path_buf();
        let version = process::run(
            process::command(&root, job.path(), "lean", &["--version"], false)?,
            job.path(),
            30,
        )?;
        std::fs::write(job.path().join("Spec.lean"), &source)?;
        self.lean(
            &root,
            job.path(),
            &["-o", "Spec.olean", "-c", "Spec.c", "Spec.lean"],
            true,
        )?;
        self.audit(&root, job.path(), "spec", &manifest.specification_proofs)?;
        let ffi_library = if manifest.abi == Abi::BuffersV1 {
            Some(crate::ffi_build::build(&root, true)?)
        } else {
            None
        };
        let ffi_bytes = ffi_library
            .as_ref()
            .map(std::fs::read)
            .transpose()?
            .unwrap_or_default();
        let metadata = serde_json::to_vec(&manifest)?;
        let id = identity(&[
            &metadata,
            source.as_bytes(),
            version.as_bytes(),
            CHECKER.as_bytes(),
            SOURCE_POLICY.as_bytes(),
            include_bytes!("toolchain.rs"),
            BRIDGE.as_bytes(),
            ENTRY.as_bytes(),
            BENCHMARK.as_bytes(),
            ABI.as_bytes(),
            BUFFER_ENTRY.as_bytes(),
            BUFFER_BRIDGE.as_bytes(),
            BUFFER_BENCHMARK.as_bytes(),
            include_bytes!("native.rs"),
            include_bytes!("buffers.rs"),
            include_bytes!("call_scope.rs"),
            &ffi_bytes,
        ]);
        // Freeze the exact adapter bytes too, not a mutable Cargo-cache filename.
        let ffi_library = ffi_library.map(|_| Arc::new(ffi_bytes));
        let mut contract = std::collections::BTreeMap::new();
        for name in [
            "Spec.olean",
            "Spec.olean.private",
            "Spec.olean.server",
            "Spec.c",
        ] {
            let file = job.path().join(name);
            if file.exists() {
                contract.insert(name.to_owned(), std::fs::read(file)?);
            }
        }
        Ok(VerifiedSpec {
            manifest,
            source,
            id,
            root,
            version,
            contract: Arc::new(contract),
            external_benchmark,
            ffi_library,
            workloads: Arc::new(workloads),
        })
    }

    /// Check a candidate against a verified contract, then compile that exact program.
    pub fn check(&self, spec: &VerifiedSpec, source: String) -> Result<VerifiedCandidate> {
        crate::source::validate(&source)?;
        let job = tempfile::tempdir()?;
        let path = job.path();
        let current = self.lean(&spec.root, path, &["--version"], false)?;
        ensure!(
            current == spec.version,
            "Lean toolchain changed after specification verification"
        );
        // Spec artifacts remain outside the candidate's writable sandbox.
        // Search path precedence prevents a candidate from shadowing Spec.olean.
        std::fs::create_dir(path.join("contract"))?;
        for (name, bytes) in spec.contract.iter() {
            std::fs::write(path.join("contract").join(name), bytes)?;
        }
        std::fs::write(path.join("Candidate.lean"), &source)?;
        self.lean(
            &spec.root,
            path,
            &[
                "-o",
                "Candidate.olean",
                "-c",
                "Candidate.c",
                "Candidate.lean",
            ],
            true,
        )?;
        for (name, bytes) in spec.contract.iter() {
            ensure!(
                std::fs::read(path.join("contract").join(name))? == *bytes,
                "candidate changed the fixed specification"
            );
        }
        std::fs::write(
            path.join("Entry.lean"),
            if spec.manifest.abi == Abi::BuffersV1 {
                BUFFER_ENTRY
            } else {
                ENTRY
            },
        )?;
        self.lean(
            &spec.root,
            path,
            &["-o", "Entry.olean", "-c", "Entry.c", "Entry.lean"],
            true,
        )?;
        let mode = if spec.manifest.abi == Abi::BuffersV1 {
            "buffers"
        } else {
            "candidate"
        };
        self.audit(&spec.root, path, mode, &spec.manifest.obligations)?;
        std::fs::write(
            path.join("bridge.c"),
            if spec.manifest.abi == Abi::BuffersV1 {
                BUFFER_BRIDGE
            } else {
                BRIDGE
            },
        )?;
        if let Some(library) = &spec.ffi_library {
            std::fs::write(path.join("contract/ffi.a"), library.as_ref())?;
        }
        let library = path.join(if cfg!(target_os = "macos") {
            "core.dylib"
        } else {
            "core.so"
        });
        let output = library.to_str().context("native library path")?;
        let mut compile = process::command(
            &spec.root,
            path,
            "leanc",
            &[
                "-shared",
                "-fPIC",
                "-O3",
                "-o",
                output,
                "contract/Spec.c",
                "Candidate.c",
                "Entry.c",
                "bridge.c",
            ],
            true,
        )?;
        if spec.ffi_library.is_some() {
            compile.arg("contract/ffi.a");
        }
        #[cfg(target_os = "macos")]
        {
            let mut xcrun = Command::new("/usr/bin/xcrun");
            xcrun.arg("--show-sdk-path");
            let sdk = process::run(xcrun, path, 30)?;
            compile.args([
                "-isysroot",
                sdk.trim(),
                "-isystem",
                &format!("{}/usr/include", sdk.trim()),
                "-Wl,-install_name,@rpath/core.dylib",
                // Rust's prebuilt std objects carry Mach-O OSO debug-map entries.
                // Their archive path includes the random candidate workspace;
                // omit that non-executable metadata so exact rebuilds match.
                "-Wl,-S",
            ]);
        }
        compile.arg(format!(
            "-Wl,-rpath,{}",
            spec.root.join("lib/lean").display()
        ));
        #[cfg(target_os = "linux")]
        compile.arg("-Wl,-soname,core.so");
        process::run(compile, path, 120)?;
        std::fs::write(
            path.join("benchmark.c"),
            if spec.manifest.abi == Abi::BuffersV1 {
                BUFFER_BENCHMARK
            } else {
                BENCHMARK
            },
        )?;
        let benchmark = path.join("benchmark");
        let mut compile = process::command(
            &spec.root,
            path,
            "leanc",
            &["-O3", "-o", "benchmark", "benchmark.c", output],
            true,
        )?;
        #[cfg(target_os = "macos")]
        {
            let mut xcrun = Command::new("/usr/bin/xcrun");
            xcrun.arg("--show-sdk-path");
            let sdk = process::run(xcrun, path, 30)?;
            compile.args([
                "-isysroot",
                sdk.trim(),
                "-isystem",
                &format!("{}/usr/include", sdk.trim()),
            ]);
        }
        compile.arg(format!("-Wl,-rpath,{}", path.display()));
        process::run(compile, path, 120)?;
        let artifact_hash = identity(&[&std::fs::read(&library)?]);
        let id = identity(&[
            spec.id.as_bytes(),
            source.as_bytes(),
            artifact_hash.as_bytes(),
        ]);
        Ok(VerifiedCandidate {
            spec: spec.clone(),
            source,
            id,
            artifact_hash,
            library,
            benchmark,
            measurements: Default::default(),
            _workspace: Arc::new(job),
        })
    }

    fn lean(&self, root: &Path, path: &Path, args: &[&str], isolated: bool) -> Result<String> {
        process::run(
            process::command(root, path, "lean", args, isolated)?,
            path,
            120,
        )
    }

    fn audit(&self, root: &Path, path: &Path, mode: &str, obligations: &[String]) -> Result<()> {
        let trusted = tempfile::tempdir()?;
        let checker = trusted.path().join("Check.lean");
        std::fs::write(&checker, CHECKER)?;
        let mut args = vec!["--run", checker.to_str().context("checker path")?, mode];
        args.extend(obligations.iter().map(String::as_str));
        let output = self.lean(root, path, &args, false)?;
        ensure!(
            output.trim() == "temper-lean:checked",
            "incomplete Lean checker output: {output}"
        );
        Ok(())
    }
}
