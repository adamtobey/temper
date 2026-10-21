//! Seeded tenant/authorization workload over the production Rust/Lean path.
//! Native compilation is outside the simulated workload; core execution is pure.

use axum::{
    body::{Body, to_bytes},
    http::{Method, Request, StatusCode},
};
use temper_authz::{AuthenticatedRequestContext, SecurityContext};
use temper_lean::{manifest::Manifest, native::ReviewedCandidate, toolchain::Toolchain};
use temper_platform::{PlatformState, lean::LeanRegistry, router::build_platform_router};
use temper_runtime::{scheduler::install_deterministic_context, tenant::TenantId};
use tower::ServiceExt;

#[tokio::test]
async fn native_core_tenant_authority_and_output_replay_across_seeds() {
    let directory =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../test-fixtures/lean/echo");
    let manifest: Manifest =
        serde_json::from_slice(&std::fs::read(directory.join("manifest.json")).unwrap()).unwrap();
    let spec = Toolchain
        .verify_spec(
            manifest,
            std::fs::read_to_string(directory.join("Spec.lean")).unwrap(),
        )
        .unwrap();
    let candidate = Toolchain
        .check(
            &spec,
            std::fs::read_to_string(directory.join("Optimized.lean")).unwrap(),
        )
        .unwrap();
    let id = candidate.id().to_string();
    let core = ReviewedCandidate::approve(candidate, "default", &id, "test-reviewer")
        .unwrap()
        .load()
        .unwrap();
    let registry = LeanRegistry::default();
    registry.install(core).unwrap();

    for seed in 0..32 {
        let mut previous = None;
        for _ in 0..2 {
            let (_guard, _, _) = install_deterministic_context(seed);
            let mut state = PlatformState::new(None);
            state.lean = registry.clone();
            for tenant in ["default", "other"] {
                state.server.authz.reload_tenant_policies(tenant, "permit(principal == Agent::\"alice\", action == Action::\"invoke_lean\", resource == LeanComponent::\"echo\");").unwrap();
            }
            let router = build_platform_router(state.clone());
            let mut random = seed + 1;
            let mut trace = Vec::new();
            for step in 0..64 {
                random ^= random << 13;
                random ^= random >> 7;
                random ^= random << 17;
                let (principal, tenant, expected) = match random % 4 {
                    0 => (None, "default", StatusCode::UNAUTHORIZED),
                    1 => (Some("bob"), "default", StatusCode::FORBIDDEN),
                    2 => (Some("alice"), "other", StatusCode::NOT_FOUND),
                    _ => (Some("alice"), "default", StatusCode::OK),
                };
                let input = format!("seed {seed} step {step}");
                let mut request = Request::builder()
                    .method(Method::POST)
                    .uri("/api/lean/echo/invoke")
                    .header("x-tenant-id", tenant)
                    .header("content-type", "application/json");
                if let Some(principal) = principal {
                    let context = AuthenticatedRequestContext::new(
                        TenantId::from(tenant),
                        SecurityContext::from_resolved_identity(principal, "test", None),
                    );
                    let token = state
                        .server
                        .internal_invocation_credentials
                        .issue_for_url(
                            context,
                            "POST",
                            "http://127.0.0.1:3000/api/lean/echo/invoke",
                        )
                        .unwrap();
                    request = request.header("authorization", format!("Bearer {token}"));
                }
                let body = serde_json::to_vec(&serde_json::json!({"input": input})).unwrap();
                let response = router
                    .clone()
                    .oneshot(request.body(Body::from(body)).unwrap())
                    .await
                    .unwrap();
                let status = response.status();
                assert_eq!(
                    status, expected,
                    "seed {seed} step {step}: authority or tenant isolation"
                );
                let output = to_bytes(response.into_body(), 65536).await.unwrap();
                if status == StatusCode::OK {
                    let output: serde_json::Value = serde_json::from_slice(&output).unwrap();
                    assert_eq!(output["output"], input, "seed {seed}: proved behavior");
                    assert_eq!(
                        output["candidate"], id,
                        "seed {seed}: artifact substitution"
                    );
                }
                trace.push((status.as_u16(), output));
            }
            if let Some(previous) = previous {
                assert_eq!(trace, previous, "seed {seed}: nondeterministic replay");
            }
            previous = Some(trace);
        }
    }
}
