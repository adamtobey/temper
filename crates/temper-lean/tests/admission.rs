//! Exercise the actual production checker, including deliberate incomplete proofs.

use temper_lean::{manifest::Manifest, toolchain::Toolchain};

fn manifest() -> Manifest {
    Manifest {
        version: 1,
        tenant: "proof-tests".into(),
        component: "echo".into(),
        toolchain: "leanprover/lean4:v4.30.0-rc2".into(),
        specification_proofs: vec!["consistent".into()],
        obligations: vec!["preserves".into()],
        objective: None,
    }
}

const SPEC: &str = "namespace Contract\nabbrev Program := String → String\ndef preserves (p : Program) : Prop := ∀ s, p s = s\ntheorem consistent : ∃ p, preserves p := ⟨id, fun _ => rfl⟩\nend Contract\n";

#[test]
fn complete_specification_must_be_checked_before_generation() {
    let spec = Toolchain.verify_spec(manifest(), SPEC.into()).unwrap();
    assert_eq!(spec.manifest().tenant, "proof-tests");
    assert_ne!(spec.id(), "");
}

#[test]
fn incomplete_or_untrusted_specifications_do_not_open_generation() {
    for proof in ["by sorry", "by exact missing", "by native_decide"] {
        let source = SPEC.replace("⟨id, fun _ => rfl⟩", proof);
        assert!(
            Toolchain.verify_spec(manifest(), source).is_err(),
            "accepted incomplete proof: {proof}"
        );
    }
    let source = SPEC
        .replace(
            "theorem consistent",
            "axiom invented : False\ntheorem consistent",
        )
        .replace("⟨id, fun _ => rfl⟩", "False.elim invented");
    assert!(Toolchain.verify_spec(manifest(), source).is_err());
}

#[test]
fn a_valid_native_computation_proof_is_outside_the_initial_axiom_policy() {
    let mut manifest = manifest();
    manifest.specification_proofs = vec!["arithmetic".into()];
    let source =
        "namespace Contract\ntheorem arithmetic : 1 + 1 = 2 := by native_decide\nend Contract\n";
    let error = Toolchain.verify_spec(manifest, source.into()).unwrap_err();
    assert!(error.to_string().contains("axiom"), "{error:#}");
}

#[test]
fn an_unlisted_incomplete_specification_proof_also_blocks_generation() {
    let source = SPEC.replace(
        "end Contract",
        "theorem unfinished : True := by sorry\nend Contract",
    );
    assert!(
        Toolchain.verify_spec(manifest(), source).is_err(),
        "a component with an unfinished helper proof was admitted"
    );
}
