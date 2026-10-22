//! Real specification -> candidate -> native Rust call, with artifact review.

use temper_lean::{
    manifest::{Direction, Manifest, Metric, Objective},
    native::ReviewedCandidate,
    toolchain::Toolchain,
};

fn inputs() -> (Manifest, String) {
    let manifest = Manifest {
        version: 1,
        tenant: "tenant-native".into(),
        component: "echo".into(),
        toolchain: "leanprover/lean4:v4.30.0-rc2".into(),
        abi: Default::default(),
        specification_proofs: vec!["consistent".into()],
        obligations: vec!["preserves".into()],
        objective: Some(Objective {
            metric: Metric::LatencyNs,
            direction: Direction::Minimize,
            inputs: vec!["hello".into(), "héllo\0world".into()],
            workloads: vec![],
            seconds: 60,
            samples: 3,
            iterations: 100,
            minimum_improvement: 0.0,
            external: None,
        }),
    };
    let spec = "namespace Contract\nabbrev Program := String → String\ndef preserves (p : Program) : Prop := ∀ s, p s = s\ntheorem consistent : ∃ p, preserves p := ⟨id, fun _ => rfl⟩\nend Contract\n";
    (manifest, spec.into())
}

const CANDIDATE: &str = "import Spec\nnamespace Candidate\ndef program (s : String) : String := s\ntheorem preserves : Contract.preserves program := fun _ => rfl\nend Candidate\n";

#[test]
fn reviewed_native_core_runs_proved_program_and_rejects_another_tenant() {
    let (manifest, source) = inputs();
    let spec = Toolchain.verify_spec(manifest, source).unwrap();
    let candidate = Toolchain.check(&spec, CANDIDATE.into()).unwrap();
    let id = candidate.id().to_string();
    assert!(ReviewedCandidate::approve(candidate.clone(), "other", &id, "reviewer").is_err());
    assert!(
        ReviewedCandidate::approve(
            candidate.clone(),
            "tenant-native",
            "different-artifact",
            "reviewer"
        )
        .is_err()
    );
    assert!(ReviewedCandidate::approve(candidate.clone(), "tenant-native", &id, "").is_err());
    let core = ReviewedCandidate::approve(candidate, "tenant-native", &id, "reviewer")
        .unwrap()
        .load()
        .unwrap();
    assert_eq!(
        core.invoke("tenant-native", "héllo\0world").unwrap(),
        "héllo\0world"
    );
    assert!(core.invoke("other", "hello").is_err());
    assert_eq!(core.reviewer(), Some("reviewer"));
}

#[test]
fn missing_wrong_and_incomplete_candidate_proofs_are_rejected() {
    let (manifest, source) = inputs();
    let spec = Toolchain.verify_spec(manifest, source).unwrap();
    for broken in [
        CANDIDATE.replace("fun _ => rfl", "by sorry"),
        CANDIDATE.replace("theorem preserves", "theorem unrelated"),
        CANDIDATE
            .replace("Contract.preserves program", "True")
            .replace("fun _ => rfl", "True.intro"),
        CANDIDATE.replace("String := s", "String := \"wrong\""),
        CANDIDATE.replace("end Candidate", "theorem unfinished : True := by sorry\nend Candidate"),
        CANDIDATE.replace("end Candidate", "def unsupported : Option (Task String) := none\nend Candidate"),
        CANDIDATE.replace("namespace Candidate", "namespace Contract\ndef preserves (_ : Program) : Prop := True\nend Contract\nnamespace Candidate"),
        CANDIDATE.replace("theorem preserves :", "axiom assumed : Contract.preserves program\ntheorem preserves :").replace("fun _ => rfl", "assumed"),
    ] {
        assert!(Toolchain.check(&spec, broken).is_err());
    }
}

#[cfg(unix)]
#[test]
fn custom_benchmark_is_snapshotted_and_measures_the_actual_artifact() {
    use sha2::{Digest, Sha256};
    let (mut manifest, source) = inputs();
    let inputs_dir = tempfile::tempdir().unwrap();
    let program = inputs_dir.path().join("measure");
    let script = b"#!/bin/sh\n/usr/bin/wc -c < \"$2\"\n";
    std::fs::write(&program, script).unwrap();
    let objective = manifest.objective.as_mut().unwrap();
    objective.metric = Metric::Custom;
    objective.external = Some(temper_lean::manifest::ExternalBenchmark {
        program: program.clone(),
        sha256: format!("{:x}", Sha256::digest(script)),
        metric: "native_artifact_bytes".into(),
    });
    let spec = Toolchain.verify_spec(manifest, source).unwrap();
    std::fs::write(program, b"changed benchmark").unwrap();
    let candidate = Toolchain.check(&spec, CANDIDATE.into()).unwrap();
    let (candidate, score) = candidate.benchmark().unwrap();
    let output = inputs_dir.path().join("saved");
    temper_lean::artifact::save(&candidate, &output).unwrap();
    assert_eq!(
        score,
        std::fs::metadata(output.join("core.native")).unwrap().len() as f64
    );
}
