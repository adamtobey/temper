//! Real Lean checking and native compilation, outside actor dispatch.

use crate::{
    identity,
    manifest::{ABI, Manifest},
    process,
};
use anyhow::{Context, Result, ensure};
use std::{
    path::{Path, PathBuf},
    process::Command,
    sync::Arc,
};

/// Complete specification, checked before any candidate is accepted.
#[derive(Clone, Debug)]
pub struct VerifiedSpec {
    pub(crate) manifest: Manifest,
    pub(crate) source: String,
    pub(crate) id: String,
    root: PathBuf,
    version: String,
    contract: Arc<tempfile::TempDir>,
    pub(crate) external_benchmark: Option<Vec<u8>>,
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

impl Toolchain {
    /// Check the complete specification before allowing implementation generation.
    pub fn verify_spec(&self, manifest: Manifest, source: String) -> Result<VerifiedSpec> {
        manifest.validate()?;
        crate::source::validate(&source)?;
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
        ]);
        Ok(VerifiedSpec {
            manifest,
            source,
            id,
            root,
            version,
            contract: Arc::new(job),
            external_benchmark,
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
        for name in [
            "Spec.olean",
            "Spec.olean.private",
            "Spec.olean.server",
            "Spec.c",
        ] {
            let source = spec.contract.path().join(name);
            if source.exists() {
                std::fs::copy(source, path.join("contract").join(name))?;
            }
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
        for name in ["Spec.olean", "Spec.c"] {
            ensure!(
                std::fs::read(path.join("contract").join(name))?
                    == std::fs::read(spec.contract.path().join(name))?,
                "candidate changed the fixed specification"
            );
        }
        std::fs::write(path.join("Entry.lean"), ENTRY)?;
        self.lean(
            &spec.root,
            path,
            &["-o", "Entry.olean", "-c", "Entry.c", "Entry.lean"],
            true,
        )?;
        self.audit(&spec.root, path, "candidate", &spec.manifest.obligations)?;
        std::fs::write(path.join("bridge.c"), BRIDGE)?;
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
            ]);
        }
        compile.arg(format!(
            "-Wl,-rpath,{}",
            spec.root.join("lib/lean").display()
        ));
        #[cfg(target_os = "linux")]
        compile.arg("-Wl,-soname,core.so");
        process::run(compile, path, 120)?;
        std::fs::write(path.join("benchmark.c"), BENCHMARK)?;
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
