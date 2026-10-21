//! Portable source/proof receipts and exact-artifact deployment review.

use crate::{
    identity,
    manifest::Manifest,
    native::ReviewedCandidate,
    toolchain::{Toolchain, VerifiedCandidate},
};
use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};
use std::path::Path;

/// Contents of a local artifact receipt. Loading always rechecks the source and
/// rebuilds the library; a JSON assertion is not proof evidence.
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Receipt {
    version: u32,
    specification: String,
    candidate: String,
    artifact_hash: String,
    manifest: Manifest,
    specification_source: String,
    implementation_source: String,
    reviewer: Option<String>,
}

/// Write a checked, unreviewed artifact to a new directory without overwriting data.
pub fn save(candidate: &VerifiedCandidate, directory: &Path) -> Result<()> {
    std::fs::create_dir(directory)?;
    std::fs::copy(&candidate.library, directory.join("core.native"))?;
    let receipt = Receipt {
        version: 1,
        specification: candidate.spec.id.clone(),
        candidate: candidate.id.clone(),
        artifact_hash: candidate.artifact_hash.clone(),
        manifest: candidate.spec.manifest.clone(),
        specification_source: candidate.spec.source.clone(),
        implementation_source: candidate.source.clone(),
        reviewer: None,
    };
    std::fs::write(
        directory.join("receipt.json"),
        serde_json::to_vec_pretty(&receipt)?,
    )?;
    Ok(())
}

fn read(directory: &Path) -> Result<(Receipt, VerifiedCandidate)> {
    let receipt: Receipt = serde_json::from_slice(&std::fs::read(directory.join("receipt.json"))?)?;
    ensure!(receipt.version == 1, "unsupported artifact receipt version");
    let bytes = std::fs::read(directory.join("core.native"))?;
    ensure!(
        identity(&[&bytes]) == receipt.artifact_hash,
        "stored native artifact was changed"
    );
    let spec = Toolchain.verify_spec(
        receipt.manifest.clone(),
        receipt.specification_source.clone(),
    )?;
    ensure!(
        spec.id() == receipt.specification,
        "stored contract/toolchain/checker mismatch"
    );
    let candidate = Toolchain.check(&spec, receipt.implementation_source.clone())?;
    ensure!(
        candidate.id() == receipt.candidate && candidate.artifact_hash() == receipt.artifact_hash,
        "stored executable does not match the checked program"
    );
    Ok((receipt, candidate))
}

/// Recheck a stored candidate without granting deployment permission.
pub fn check(directory: &Path) -> Result<VerifiedCandidate> {
    Ok(read(directory)?.1)
}

/// Record explicit local deployment review for an exact candidate. The host must
/// authenticate the reviewer before calling this administrative operation.
pub fn review(directory: &Path, expected_id: &str, reviewer: &str) -> Result<()> {
    let (mut receipt, candidate) = read(directory)?;
    ReviewedCandidate::approve(candidate, &receipt.manifest.tenant, expected_id, reviewer)?;
    receipt.reviewer = Some(reviewer.into());
    let mut temporary = tempfile::NamedTempFile::new_in(directory)?;
    use std::io::Write;
    temporary.write_all(&serde_json::to_vec_pretty(&receipt)?)?;
    temporary.persist(directory.join("receipt.json"))?;
    Ok(())
}

/// Load a local operator-controlled artifact directory after proof/byte checks.
/// Unreviewed artifacts cannot be loaded through this deployment entry point.
pub fn load_reviewed(directory: &Path) -> Result<ReviewedCandidate> {
    let (receipt, candidate) = read(directory)?;
    let reviewer = receipt.reviewer.as_deref().filter(|s| !s.is_empty());
    let reviewer = reviewer.ok_or_else(|| anyhow::anyhow!("deployment review is required"))?;
    ReviewedCandidate::approve(
        candidate,
        &receipt.manifest.tenant,
        &receipt.candidate,
        reviewer,
    )
}
