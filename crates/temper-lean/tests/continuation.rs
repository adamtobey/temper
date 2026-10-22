//! Search must survive empty generations, provider faults and rejected proofs.
use anyhow::Result;
use temper_lean::{
    manifest::Manifest,
    search::{Attempt, Episode, Proposer},
    toolchain::{Toolchain, VerifiedCandidate, VerifiedSpec},
};

struct Scripted {
    calls: usize,
}

impl Proposer for Scripted {
    fn propose(
        &mut self,
        _: &VerifiedSpec,
        _: Option<&VerifiedCandidate>,
        _: &[Attempt],
    ) -> Result<Option<String>> {
        self.calls += 1;
        match self.calls {
            1 => Ok(None),
            2 => anyhow::bail!("transient provider error"),
            3 => Ok(Some("not a Lean implementation".into())),
            _ => Ok(Some("import Spec\nnamespace Candidate\ndef program (s : String) := s\ntheorem preserves : Contract.preserves program := by intro s; rfl\nend Candidate".into())),
        }
    }
}

#[test]
fn empty_or_failed_proposals_do_not_end_a_budgeted_search() {
    let manifest: Manifest = serde_json::from_str(include_str!(
        "../../../test-fixtures/lean/echo/manifest.json"
    ))
    .unwrap();
    let spec = Toolchain
        .verify_spec(
            manifest,
            include_str!("../../../test-fixtures/lean/echo/Spec.lean").into(),
        )
        .unwrap();
    let mut episode = Episode::new(&spec, 6).unwrap();
    let mut proposer = Scripted { calls: 0 };
    episode.run(&mut proposer).unwrap();
    assert_eq!(
        proposer.calls, 6,
        "an empty result must not stop exploration"
    );
    assert_eq!(episode.history().len(), 6);
    assert!(episode.best().is_some(), "search must recover after faults");
}

#[test]
fn binary_batch_abi_is_an_explicit_checked_manifest_choice() {
    let mut manifest: serde_json::Value = serde_json::from_str(include_str!(
        "../../../test-fixtures/lean/echo/manifest.json"
    ))
    .unwrap();
    manifest["abi"] = "buffers_v1".into();
    assert!(serde_json::from_value::<Manifest>(manifest).is_ok());
}

#[test]
fn search_explores_verified_alternatives_after_stalls() {
    let manifest: Manifest = serde_json::from_str(include_str!(
        "../../../test-fixtures/lean/echo/manifest.json"
    ))
    .unwrap();
    let spec = Toolchain
        .verify_spec(
            manifest,
            include_str!("../../../test-fixtures/lean/echo/Spec.lean").into(),
        )
        .unwrap();
    let mut episode = Episode::new(&spec, 8).unwrap();
    let slow = episode
        .submit(include_str!("../../../test-fixtures/lean/echo/Baseline.lean").into())
        .unwrap()
        .candidate
        .clone()
        .unwrap();
    assert!(
        episode
            .submit(include_str!("../../../test-fixtures/lean/echo/Optimized.lean").into())
            .unwrap()
            .accepted
    );
    let mut proposer = Scripted { calls: 0 };
    episode.run(&mut proposer).unwrap();
    assert_eq!(
        episode.history()[5].parent.as_deref(),
        Some(slow.as_str()),
        "after three failed attempts, explore a verified alternative without losing the incumbent"
    );
    assert_eq!(episode.history().len(), 8);
}

#[test]
fn cosmetic_changes_cannot_win_by_benchmark_noise() {
    let manifest: Manifest = serde_json::from_str(include_str!(
        "../../../test-fixtures/lean/echo/manifest.json"
    ))
    .unwrap();
    let spec = Toolchain
        .verify_spec(
            manifest,
            include_str!("../../../test-fixtures/lean/echo/Spec.lean").into(),
        )
        .unwrap();
    let source = include_str!("../../../test-fixtures/lean/echo/Optimized.lean");
    let mut episode = Episode::new(&spec, 2).unwrap();
    assert!(episode.submit(source.into()).unwrap().accepted);
    let cosmetic = episode
        .submit(format!("-- cosmetic change only\n{source}"))
        .unwrap();
    assert!(!cosmetic.accepted && cosmetic.measurement.is_none());
    assert!(
        cosmetic
            .diagnostic
            .as_deref()
            .unwrap()
            .contains("identical native executable")
    );
}

#[test]
fn duplicate_workload_names_cannot_erase_a_profile_from_scoring() {
    let manifest: Manifest = serde_json::from_value(serde_json::json!({
        "version":1,"tenant":"default","component":"profiles","abi":"buffers_v1",
        "toolchain":"leanprover/lean4:v4.30.0-rc2","specification_proofs":["consistent"],"obligations":["preserves"],
        "objective":{"metric":"cpu_ns","direction":"minimize","inputs":[],"samples":3,"iterations":1,"minimum_improvement":0.02,
            "workloads":[
                {"name":"same","parameter":1,"files":[{"path":"/tmp/input.raw","sha256":"0000000000000000000000000000000000000000000000000000000000000000"}]},
                {"name":"same","parameter":2,"files":[{"path":"/tmp/input.raw","sha256":"0000000000000000000000000000000000000000000000000000000000000000"}]}
            ]}
    })).unwrap();
    assert!(
        manifest
            .validate()
            .unwrap_err()
            .to_string()
            .contains("duplicate binary workload name")
    );
}
