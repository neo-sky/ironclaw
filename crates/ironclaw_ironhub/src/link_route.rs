use std::num::{NonZeroU32, NonZeroU64};
use std::sync::Arc;

use axum::Router;
use axum::body::Bytes;
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::post;
use ironclaw_host_api::ingress::{
    AllowedEffectPath, AuditTraceClass, BodyLimitPolicy, CorsPolicy, IngressAuthPolicy,
    IngressAuthScheme, IngressPolicy, IngressPolicyParts, IngressRouteDescriptor,
    IngressScopeSource, ListenerClass, RateLimitPolicy, RateLimitScope, StreamingMode,
    WebSocketOriginPolicy,
};
use ironclaw_host_api::{NetworkMethod, UserId};
use ironclaw_host_ingress::PublicRouteMount;

use crate::link::{
    IronhubInstallDeliveryRequest, IronhubLinkError, IronhubLinkService, IronhubRegisterRequest,
};

const REGISTER_PATH: &str = "/api/ironhub/register";
const INSTALL_PATH: &str = "/api/ironhub/install";
const BODY_LIMIT_BYTES: NonZeroU64 = NonZeroU64::new(8 * 1024).expect("8192 != 0"); // safety: const-evaluated, literal non-zero
const MAX_REQUESTS: NonZeroU32 = NonZeroU32::new(600).expect("600 != 0"); // safety: const-evaluated, literal non-zero
const RATE_WINDOW_SECONDS: NonZeroU32 = NonZeroU32::new(60).expect("60 != 0"); // safety: const-evaluated, literal non-zero

#[derive(Clone)]
struct RouteState {
    link: Arc<dyn IronhubLinkService>,
    owner: UserId,
}

impl std::fmt::Debug for RouteState {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.debug_struct("IronhubLinkRouteState").finish()
    }
}

pub fn link_route_mount(link: Arc<dyn IronhubLinkService>, owner: UserId) -> PublicRouteMount {
    let state = RouteState { link, owner };
    PublicRouteMount::new(
        Router::new()
            .route(REGISTER_PATH, post(register_handler))
            .route(INSTALL_PATH, post(install_handler))
            .with_state(state),
        vec![
            descriptor("ironhub.register", REGISTER_PATH),
            descriptor("ironhub.install", INSTALL_PATH),
        ],
    )
}

fn descriptor(route_id: &str, path: &str) -> IngressRouteDescriptor {
    IngressRouteDescriptor::new(route_id, NetworkMethod::Post, path, policy())
        .expect("ironhub deep-link route descriptor must validate at startup") // safety: ids/patterns are crate-local literals and the policy is built by the sibling helper below.
}

fn policy() -> IngressPolicy {
    IngressPolicy::new(IngressPolicyParts {
        listener_class: ListenerClass::PublicWebhook,
        auth: IngressAuthPolicy::Required {
            schemes: vec![IngressAuthScheme::WebhookSignature],
        },
        scope_source: IngressScopeSource::HostResolved,
        body_limit: BodyLimitPolicy::Limited {
            max_bytes: BODY_LIMIT_BYTES,
        },
        rate_limit: RateLimitPolicy::Limited {
            scope: RateLimitScope::Global,
            max_requests: MAX_REQUESTS,
            window_seconds: RATE_WINDOW_SECONDS,
        },
        cors: CorsPolicy::NotApplicable,
        websocket_origin: WebSocketOriginPolicy::NotApplicable,
        streaming: StreamingMode::None,
        audit: AuditTraceClass::PublicCallback,
        effect_path: AllowedEffectPath::ProductSurface,
    })
    .expect("ironhub deep-link ingress policy must validate at startup") // safety: every policy part is a crate-local literal validated by its own constructor.
}

fn status_for(error: &IronhubLinkError) -> StatusCode {
    match error {
        IronhubLinkError::InvalidSignature
        | IronhubLinkError::StaleTimestamp
        | IronhubLinkError::Replay => StatusCode::FORBIDDEN,
        IronhubLinkError::InvalidInput { .. } => StatusCode::BAD_REQUEST,
        IronhubLinkError::Unavailable => StatusCode::SERVICE_UNAVAILABLE,
        IronhubLinkError::Install { .. } => StatusCode::INTERNAL_SERVER_ERROR,
    }
}

async fn register_handler(State(state): State<RouteState>, body: Bytes) -> Response {
    let request: IronhubRegisterRequest = match serde_json::from_slice(&body) {
        Ok(request) => request,
        Err(_) => return StatusCode::BAD_REQUEST.into_response(),
    };
    match state.link.register(request).await {
        Ok(()) => StatusCode::OK.into_response(),
        Err(error) => status_for(&error).into_response(),
    }
}

async fn install_handler(State(state): State<RouteState>, body: Bytes) -> Response {
    let request: IronhubInstallDeliveryRequest = match serde_json::from_slice(&body) {
        Ok(request) => request,
        Err(_) => return StatusCode::BAD_REQUEST.into_response(),
    };
    match state
        .link
        .deliver_install(state.owner.clone(), request)
        .await
    {
        Ok(result) => match serde_json::to_vec(&result) {
            Ok(json) => ([("content-type", "application/json")], json).into_response(),
            Err(_) => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
        },
        Err(error) => status_for(&error).into_response(),
    }
}
