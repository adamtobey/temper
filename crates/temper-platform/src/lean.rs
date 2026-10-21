//! Reviewed native Lean cores behind the existing tenant and Cedar boundaries.

use crate::PlatformState;
use axum::{
    Extension, Json, Router,
    extract::{Path, State},
    http::StatusCode,
    routing::post,
};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    sync::{Arc, RwLock},
};
use temper_authz::AuthenticatedRequestContext;
use temper_lean::native::NativeCore;
use temper_runtime::tenant::TenantId;

/// Installed, reviewed cores, explicitly keyed by tenant and component.
#[derive(Clone, Default)]
pub struct LeanRegistry(Arc<RwLock<CoreMap>>);

type CoreMap = BTreeMap<(TenantId, String), Arc<NativeCore>>;

impl LeanRegistry {
    /// Install an already reviewed native core; never accepts raw candidate code.
    pub fn install(&self, core: NativeCore) -> anyhow::Result<()> {
        let key = (TenantId::from(core.tenant()), core.component().to_owned());
        self.0
            .write()
            .map_err(|_| anyhow::anyhow!("Lean registry poisoned"))?
            .insert(key, Arc::new(core));
        Ok(())
    }

    fn get(
        &self,
        tenant: &TenantId,
        component: &str,
    ) -> Result<Option<Arc<NativeCore>>, StatusCode> {
        let registry = self
            .0
            .read()
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
        Ok(registry.get(&(tenant.clone(), component.into())).cloned())
    }
}

/// Invocation envelope. Rust services own state and external effects.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Invoke {
    /// UTF-8 request for the pure Lean core, conventionally JSON.
    pub input: String,
}

/// Response identifies the exact reviewed implementation that produced it.
#[derive(Serialize)]
pub struct Invoked {
    /// Output from the checked core.
    pub output: String,
    /// Content identity of the executed candidate and native artifact.
    pub candidate: String,
}

/// Native core service routes; platform middleware supplies credential identity.
pub fn router() -> Router<PlatformState> {
    Router::new().route("/api/lean/{component}/invoke", post(invoke))
}

async fn invoke(
    State(state): State<PlatformState>,
    authenticated: Option<Extension<AuthenticatedRequestContext>>,
    Path(component): Path<String>,
    Json(request): Json<Invoke>,
) -> Result<Json<Invoked>, (StatusCode, String)> {
    let authenticated =
        authenticated.ok_or((StatusCode::UNAUTHORIZED, "authentication required".into()))?;
    let tenant = authenticated.tenant();
    temper_server::authz::require_governed_authorization(
        &state.server,
        &authenticated,
        "invoke_lean",
        "LeanComponent",
        &component,
    )
    .await?;
    let core = state
        .lean
        .get(tenant, &component)
        .map_err(|s| (s, "Lean registry unavailable".into()))?
        .ok_or((
            StatusCode::NOT_FOUND,
            "Lean component not found in tenant".into(),
        ))?;
    let output = core
        .invoke(tenant.as_str(), &request.input)
        .map_err(|error| {
            tracing::error!(%error, %tenant, %component, "native Lean invocation failed");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                "Lean invocation failed".into(),
            )
        })?;
    tracing::info!(%tenant, %component, candidate = core.id(), "reviewed Lean core invoked");
    Ok(Json(Invoked {
        output,
        candidate: core.id().into(),
    }))
}
