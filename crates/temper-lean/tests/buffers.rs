//! Actual lean-ffi bridge: binary data, typed errors, thread ownership and review.
use temper_lean::{artifact, manifest::Manifest, toolchain::Toolchain};

const SPEC: &str = r#"namespace Contract
abbrev Program := UInt64 → Array ByteArray → Except String (Array ByteArray)
def reference (code : UInt64) (files : Array ByteArray) : Except String (Array ByteArray) :=
  if code = 0 then .error "error\x00☃" else .ok files
def preserves (p : Program) : Prop := ∀ code files, p code files = reference code files
theorem consistent : ∃ p, preserves p := ⟨reference, fun _ _ => rfl⟩
end Contract
"#;
const CANDIDATE: &str = "import Spec\nnamespace Candidate\ndef program := Contract.reference\ntheorem preserves : Contract.preserves program := fun _ _ => rfl\nend Candidate\n";

#[test]
fn typed_buffers_are_lossless_scoped_and_reviewed() {
    use sha2::{Digest, Sha256};
    let workload = tempfile::tempdir().unwrap();
    let path = workload.path().join("input.bin");
    let data: Vec<_> = (0..1024 * 1024).map(|i| (i % 256) as u8).collect();
    std::fs::write(&path, &data).unwrap();
    let manifest: Manifest = serde_json::from_value(serde_json::json!({
        "version":1,"tenant":"binary-tenant","component":"binary-echo",
        "toolchain":"leanprover/lean4:v4.30.0-rc2","abi":"buffers_v1",
        "specification_proofs":["consistent"],"obligations":["preserves"],
        "objective":{"metric":"cpu_ns","direction":"minimize","inputs":[],
            "workloads":[{"name":"binary-copy","parameter":1,"files":[{"path":path,"sha256":format!("{:x}", Sha256::digest(&data))}]}],
            "samples":3,"iterations":10,"minimum_improvement":0.0},
    })).unwrap();
    let spec = Toolchain
        .verify_spec(manifest.clone(), SPEC.into())
        .unwrap();
    std::fs::write(&path, b"changed after the proof gate").unwrap();
    assert!(
        Toolchain.verify_spec(manifest, SPEC.into()).is_err(),
        "changed inputs must not silently change the objective"
    );
    let candidate = Toolchain.check(&spec, CANDIDATE.into()).unwrap();
    let cosmetic = Toolchain
        .check(&spec, format!("-- no executable change\n{CANDIDATE}"))
        .unwrap();
    assert_eq!(candidate.artifact_hash(), cosmetic.artifact_hash());
    let (candidate, score) = candidate.benchmark().unwrap();
    assert!(
        score.is_finite() && score > 0.0,
        "real snapshotted CPU benchmark did not execute"
    );
    let id = candidate.id().to_string();
    let stored = workload.path().join("stored");
    temper_lean::artifact::save(&candidate, &stored).unwrap();
    // The package pins files: restore them for a fresh gate, then prove that
    // persistence/reload reproduces the exact binary before any review.
    std::fs::write(&path, &data).unwrap();
    assert_eq!(temper_lean::artifact::check(&stored).unwrap().id(), id);
    artifact::review(&stored, &id, "test-only-reviewer").unwrap();
    let core = artifact::load_reviewed(&stored).unwrap().load().unwrap();
    for _ in 0..50 {
        core.with_buffers(
            "binary-tenant",
            u64::MAX,
            &[&data, &[], &[0, 255, 128]],
            |result| {
                let buffers = result.unwrap();
                assert_eq!(buffers, &[data.as_slice(), &[], &[0, 255, 128]]);
            },
        )
        .unwrap();
    }
    assert!(core.invoke("binary-tenant", "not binary").is_err());
    assert!(core.invoke_buffers("other", 1, &[&data]).is_err());
    assert!(
        core.invoke_buffers("binary-tenant", 0, &[])
            .unwrap_err()
            .to_string()
            .contains("☃")
    );
    core.with_buffers("binary-tenant", 0, &[], |result| {
        assert_eq!(result.unwrap_err(), "error\0☃");
    })
    .unwrap();
    assert!(
        core.invoke_buffers("binary-tenant", 1, &[])
            .unwrap()
            .is_empty()
    );
    core.with_buffers("binary-tenant", 1, &[&data], |_| {
        assert!(
            core.invoke_buffers("binary-tenant", 1, &[])
                .unwrap_err()
                .to_string()
                .contains("reentrant")
        );
        assert!(
            core.invoke("binary-tenant", "nested")
                .unwrap_err()
                .to_string()
                .contains("reentrant")
        );
    })
    .unwrap();
    // Real native code on foreign Rust threads, serialized by the production host.
    std::thread::scope(|scope| {
        for _ in 0..4 {
            let core = &core;
            scope.spawn(move || {
                for _ in 0..20 {
                    assert_eq!(
                        core.invoke_buffers("binary-tenant", 1, &[&[0, 255]])
                            .unwrap(),
                        vec![vec![0, 255]]
                    );
                }
            });
        }
    });
}
