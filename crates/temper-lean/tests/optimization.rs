//! End-to-end fixed-contract optimization using actual Lean proofs and benchmarks.

use temper_lean::{artifact, manifest::Manifest, search::Episode, toolchain::Toolchain};

const PACKAGE: &str = "../../test-fixtures/lean/echo/";

#[test]
fn benchmarked_improvement_advances_search_without_review_and_survives_reload() {
    let directory = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(PACKAGE);
    let manifest: Manifest =
        serde_json::from_slice(&std::fs::read(directory.join("manifest.json")).unwrap()).unwrap();
    let spec = Toolchain
        .verify_spec(
            manifest,
            std::fs::read_to_string(directory.join("Spec.lean")).unwrap(),
        )
        .unwrap();
    let mut episode = Episode::new(&spec, 3).unwrap();
    let baseline = std::fs::read_to_string(directory.join("Baseline.lean")).unwrap();
    let first = episode.submit(baseline).unwrap().clone();
    assert!(first.accepted, "baseline: {:?}", first.diagnostic);
    let broken = episode
        .submit(
            "import Spec\nnamespace Candidate\ndef program (s : String) := s\nend Candidate\n"
                .into(),
        )
        .unwrap();
    assert!(!broken.accepted);
    assert!(broken.diagnostic.is_some());
    assert_eq!(
        episode.best().unwrap().id(),
        first.candidate.as_deref().unwrap()
    );
    let optimized = std::fs::read_to_string(directory.join("Optimized.lean")).unwrap();
    let improvement = episode.submit(optimized).unwrap().clone();
    assert!(
        improvement.accepted,
        "improvement: {improvement:?}, baseline: {first:?}"
    );
    assert_eq!(improvement.parent, first.candidate);
    assert!(
        episode.submit("irrelevant".into()).is_err(),
        "search budget was ignored"
    );
    let output = tempfile::tempdir().unwrap();
    let saved = output.path().join("artifact");
    artifact::save(episode.best().unwrap(), &saved).unwrap();
    assert!(
        artifact::load_reviewed(&saved).is_err(),
        "unreviewed artifact deployed"
    );
    let id = improvement.candidate.unwrap();
    artifact::review(&saved, &id, "test-reviewer").unwrap();
    let core = artifact::load_reviewed(&saved).unwrap().load().unwrap();
    assert_eq!(
        core.invoke("default", "reviewed output").unwrap(),
        "reviewed output"
    );
    std::fs::write(saved.join("core.native"), b"changed binary").unwrap();
    assert!(
        artifact::load_reviewed(&saved).is_err(),
        "artifact substitution accepted"
    );
}
