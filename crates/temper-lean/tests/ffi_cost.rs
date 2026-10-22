//! Manual, real-API transport comparison. No data generation is timed.
use temper_lean::{
    manifest::Manifest,
    native::{NativeCore, ReviewedCandidate},
    toolchain::Toolchain,
};

fn binary() -> NativeCore {
    let manifest: Manifest = serde_json::from_value(serde_json::json!({
        "version":1,"tenant":"default","component":"ffi-cost","abi":"buffers_v1",
        "toolchain":"leanprover/lean4:v4.30.0-rc2","specification_proofs":["consistent"],
        "obligations":["preserves"],"objective":null
    }))
    .unwrap();
    let source = "namespace Contract\nabbrev Program := UInt64 → Array ByteArray → Except String (Array ByteArray)\ndef preserves (p : Program) : Prop := ∀ n b, p n b = .ok b\ntheorem consistent : ∃ p, preserves p := ⟨fun _ b => .ok b, fun _ _ => rfl⟩\nend Contract\n";
    let spec = Toolchain.verify_spec(manifest, source.into()).unwrap();
    let candidate = Toolchain.check(&spec, "import Spec\nnamespace Candidate\ndef program (_ : UInt64) (b : Array ByteArray) : Except String (Array ByteArray) := .ok b\ntheorem preserves : Contract.preserves program := fun _ _ => rfl\nend Candidate\n".into()).unwrap();
    let id = candidate.id().to_owned();
    ReviewedCandidate::approve(candidate, "default", &id, "ffi-cost-local-test")
        .unwrap()
        .load()
        .unwrap()
}

fn string() -> NativeCore {
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
    let candidate = Toolchain
        .check(
            &spec,
            include_str!("../../../test-fixtures/lean/echo/Optimized.lean").into(),
        )
        .unwrap();
    let id = candidate.id().to_owned();
    ReviewedCandidate::approve(candidate, "default", &id, "ffi-cost-local-test")
        .unwrap()
        .load()
        .unwrap()
}

fn cpu_ns() -> f64 {
    let mut time = std::mem::MaybeUninit::<libc::timespec>::uninit();
    assert_eq!(
        unsafe { libc::clock_gettime(libc::CLOCK_PROCESS_CPUTIME_ID, time.as_mut_ptr()) },
        0
    );
    let time = unsafe { time.assume_init() };
    time.tv_sec as f64 * 1e9 + time.tv_nsec as f64
}

#[test]
#[ignore = "manual CPU measurements, not a flaky speed assertion"]
fn compare_native_identity_boundaries() {
    let text_core = string();
    let binary_core = binary();
    for size in [64, 1024 * 1024] {
        let data = vec![b'x'; size];
        let text = std::str::from_utf8(&data).unwrap();
        let iterations = if size == 64 { 10000 } else { 100 };
        let mut samples = [Vec::new(), Vec::new()];
        let invoke = |mode| {
            if mode == 0 {
                std::hint::black_box(text_core.invoke("default", text).unwrap().len());
            } else {
                binary_core
                    .with_buffers("default", 1, &[&data], |result| {
                        let buffers = result.unwrap();
                        assert_eq!(buffers[0].len(), size);
                        std::hint::black_box(buffers[0][0]);
                    })
                    .unwrap();
            }
        };
        for _ in 0..10 {
            invoke(0);
            invoke(1);
        }
        for sample in 0..7 {
            for offset in 0..2 {
                let mode = (sample + offset) % 2;
                let start = cpu_ns();
                for _ in 0..iterations {
                    invoke(mode);
                }
                samples[mode].push((cpu_ns() - start) / iterations as f64);
            }
        }
        for values in &mut samples {
            values.sort_by(f64::total_cmp);
        }
        println!(
            "{{\"bytes\":{size},\"string_cpu_ns\":{},\"borrowed_buffers_cpu_ns\":{},\"samples\":7,\"generation_timed\":false}}",
            samples[0][3], samples[1][3]
        );
    }
}
