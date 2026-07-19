//! OpenAPI 3.1 specification for the BeeAI ACP run surface (+ probe routes).

use axum::routing::get;
use axum::{Json, Router};
use utoipa::openapi::security::{Http, HttpAuthScheme, SecurityScheme};
use utoipa::{Modify, OpenApi};

use super::model::{
    ApprovalDecision, ApprovalKind, HealthResponse, LamportDelta, Message, MessagePart,
    ProtocolErrorBody, ProtocolErrorDetail, ReadyResponse, Run, RunApproval, RunStatus,
    TransactionPreview,
};
use super::routes::{CreateRunRequest, ResumeRunRequest};

// Stub handlers exist only so `#[utoipa::path]` can register probe routes in the
// OpenAPI document; the real handlers live on the fleet serve router.
#[allow(dead_code)]
#[utoipa::path(
    get,
    path = "/health",
    tag = "probes",
    responses(
        (status = 200, description = "Process liveness", body = HealthResponse),
    )
)]
fn health_doc() {}

#[allow(dead_code)]
#[utoipa::path(
    get,
    path = "/ready",
    tag = "probes",
    responses(
        (status = 200, description = "Dependencies ready", body = ReadyResponse),
        (status = 503, description = "Not ready", body = ReadyResponse),
    )
)]
fn ready_doc() {}

struct SecurityAddon;

impl Modify for SecurityAddon {
    fn modify(&self, openapi: &mut utoipa::openapi::OpenApi) {
        let components = openapi.components.get_or_insert_with(Default::default);
        components.add_security_scheme(
            "bearer_auth",
            SecurityScheme::Http(Http::new(HttpAuthScheme::Bearer)),
        );
    }
}

#[derive(OpenApi)]
#[openapi(
    info(
        title = "Altius BeeAI ACP",
        version = "0.1.0",
        description = "REST run lifecycle for the Altius fleet (BeeAI Agent Communication Protocol). \
            Runs are single-shot units (`created → in-progress → awaiting ⇄ in-progress → terminal`). \
            Human approval pauses emit a typed `approval` object on run snapshots and SSE `event: run` frames. \
            Authentication: optional `Authorization: Bearer <token>` on all `/runs*` routes; \
            SSE clients may pass `?token=` on `/runs/{id}/events` when headers are unavailable."
    ),
    paths(
        health_doc,
        ready_doc,
        crate::beeacp::routes::list_runs,
        crate::beeacp::routes::create_run,
        crate::beeacp::routes::get_run,
        crate::beeacp::routes::cancel_run,
        crate::beeacp::routes::resume_run,
        crate::beeacp::routes::run_events,
    ),
    components(schemas(
        Run,
        RunStatus,
        RunApproval,
        ApprovalKind,
        ApprovalDecision,
        TransactionPreview,
        LamportDelta,
        Message,
        MessagePart,
        CreateRunRequest,
        ResumeRunRequest,
        ProtocolErrorBody,
        ProtocolErrorDetail,
        HealthResponse,
        ReadyResponse,
    )),
    modifiers(&SecurityAddon),
    tags(
        (name = "runs", description = "BeeAI ACP run lifecycle"),
        (name = "probes", description = "Liveness and readiness (unauthenticated)"),
    ),
    security(("bearer_auth" = []))
)]
pub struct BeeAcpApiDoc;

/// Serialized OpenAPI document (OpenAPI 3.1).
pub fn openapi_spec() -> utoipa::openapi::OpenApi {
    BeeAcpApiDoc::openapi()
}

async fn serve_openapi() -> Json<utoipa::openapi::OpenApi> {
    Json(openapi_spec())
}

/// Public router exposing machine-readable API documentation.
pub fn openapi_router() -> Router {
    Router::new().route("/openapi.json", get(serve_openapi))
}

#[cfg(test)]
mod tests {
    use super::*;
    use utoipa::openapi::OpenApiVersion;

    #[test]
    fn spec_is_openapi_3_1_with_run_paths() {
        let spec = openapi_spec();
        assert!(
            matches!(spec.openapi, OpenApiVersion::Version31),
            "expected OpenAPI 3.1"
        );
        let json = serde_json::to_value(&spec).expect("serialize openapi");
        let paths = json["paths"].as_object().expect("paths");
        for key in [
            "/runs",
            "/runs/{id}",
            "/runs/{id}/events",
            "/runs/{id}/cancel",
            "/health",
            "/ready",
        ] {
            assert!(paths.contains_key(key), "missing path {key}");
        }
        assert!(json["components"]["schemas"]
            .as_object()
            .expect("schemas")
            .contains_key("RunApproval"));
    }
}
