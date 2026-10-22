//! Build only trusted, pinned Rust adapter sources, outside candidate isolation.
//! Ordinary Temper builds remain independent of the Lean and libclang installs.
use crate::{identity, process};
use anyhow::{Context, Result, ensure};
use std::{
    path::{Path, PathBuf},
    process::Command,
};

pub(crate) const SOURCE: &str = include_str!("../bridge/bridge.rs");
pub(crate) const MANIFEST: &str = include_str!("../bridge/Cargo.toml");
pub(crate) const LOCK: &str = include_str!("../bridge/Cargo.lock");

pub(crate) fn build(root: &Path, offline: bool) -> Result<PathBuf> {
    let cargo = std::env::var_os("TEMPER_LEAN_CARGO")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            let compiled = PathBuf::from(env!("CARGO"));
            if compiled.is_file() {
                compiled
            } else {
                PathBuf::from("cargo")
            }
        });
    let rustc = std::env::var_os("TEMPER_LEAN_RUSTC")
        .map(PathBuf::from)
        .unwrap_or_else(|| cargo.with_file_name("rustc"));
    let mut version = Command::new(&rustc);
    version.arg("--version");
    let rust_version = process::run(version, &std::env::temp_dir(), 30)?;
    let key = identity(&[
        SOURCE.as_bytes(),
        MANIFEST.as_bytes(),
        LOCK.as_bytes(),
        root.to_string_lossy().as_bytes(),
        rust_version.as_bytes(),
    ]);
    let directory = std::env::temp_dir().join(format!("temper-lean-ffi-{key}"));
    #[cfg(unix)]
    {
        use std::os::unix::fs::{DirBuilderExt, MetadataExt};
        match std::fs::DirBuilder::new().mode(0o700).create(&directory) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(e) => return Err(e.into()),
        }
        let meta = std::fs::symlink_metadata(&directory)?;
        ensure!(
            meta.is_dir() && meta.uid() == unsafe { libc::geteuid() },
            "untrusted FFI build cache"
        );
    }
    for (name, bytes) in [
        ("Cargo.toml", MANIFEST),
        ("Cargo.lock", LOCK),
        ("bridge.rs", SOURCE),
    ] {
        let file = directory.join(name);
        // Don't change mtimes and invalidate Cargo's cache on every candidate.
        if std::fs::read(&file).ok().as_deref() != Some(bytes.as_bytes()) {
            ensure!(
                !std::fs::symlink_metadata(&file).is_ok_and(|m| m.file_type().is_symlink()),
                "symlink in FFI cache"
            );
            use std::io::Write;
            let mut temporary = tempfile::NamedTempFile::new_in(&directory)?;
            temporary.write_all(bytes.as_bytes())?;
            temporary.persist(file)?;
        }
    }
    let mut command = Command::new(cargo);
    command
        .args(["build", "--release", "--locked", "--manifest-path"])
        .arg(directory.join("Cargo.toml"))
        .arg("--target-dir")
        .arg(directory.join("target"))
        .env("LEAN_SYSROOT", root)
        .env("RUSTC", rustc);
    if offline {
        command.arg("--offline");
    }
    process::run(command, &directory, 300).context(
        "build pinned lean-ffi adapter (run `temper lean setup-ffi` to prepare dependencies)",
    )?;
    let library = directory.join("target/release/libtemper_lean_ffi_bridge.a");
    ensure!(
        library.is_file(),
        "Rust FFI adapter build produced no static library"
    );
    Ok(library)
}
