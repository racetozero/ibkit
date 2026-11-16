//! Control API: REST and WebSocket on `control_listen`, protected by a
//! bearer token. Commands go through the same ordered command queue as the
//! CLI, so the run loop stays the only owner of the Gateway process.
use crate::{
    Config, Event,
    api::{
        ApiError, ConfigUpdate, ControlRequest, CredentialsRequest, Health, Status, TotpRequest,
    },
    gateway::{api_handshake, check_config, read_status, send_control, write_private},
    settings,
};
use anyhow::Result;
use axum::{
    Json, Router,
    extract::{
        Request, State,
        ws::{Message, WebSocket, WebSocketUpgrade},
    },
    http::{StatusCode, header},
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::{get, post, put},
};
use std::{fmt::Display, fs, path::PathBuf, sync::Arc};
use tokio::{net::TcpListener, sync::broadcast};

#[derive(Clone)]
pub struct ControlApi {
    pub token: Arc<str>,
    pub state_dir: PathBuf,
    /// The config file the controller reads, if any.
    pub config_path: Option<PathBuf>,
    /// Settings from CLI flags and IBKIT_* variables; the API cannot change them.
    pub flags: Arc<toml::Table>,
    pub events: broadcast::Sender<Event>,
    pub metrics: Arc<crate::monitor::Metrics>,
}

type Failure = (StatusCode, Json<ApiError>);

fn fail(code: StatusCode, error: impl Display) -> Failure {
    (
        code,
        Json(ApiError {
            error: format!("{error:#}"),
        }),
    )
}

/// Serve the control API until the listener fails.
pub async fn serve(listener: TcpListener, api: ControlApi) -> Result<()> {
    let protected = Router::new()
        .route("/v1/status", get(status))
        .route("/v1/control", post(control))
        .route(
            "/v1/config",
            get(config).patch(update_config).delete(clear_config),
        )
        .route("/v1/credentials", put(credentials))
        .route("/v1/totp", put(set_totp).delete(clear_totp))
        .route("/v1/events", get(events))
        .route("/v1/metrics", get(metrics))
        .route_layer(middleware::from_fn_with_state(api.clone(), authorize));
    let app = Router::new()
        .route("/v1/health", get(health))
        .route("/v1/health/ready", get(ready))
        .route("/v1/openapi.json", get(openapi))
        .merge(protected)
        .with_state(api);
    axum::serve(listener, app).await?;
    Ok(())
}

async fn authorize(State(api): State<ControlApi>, request: Request, next: Next) -> Response {
    let given = request
        .headers()
        .get(header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "))
        .unwrap_or("")
        .as_bytes();
    let expected = api.token.as_bytes();
    // Constant time for tokens of the expected length.
    let valid = given.len() == expected.len()
        && given
            .iter()
            .zip(expected)
            .fold(0, |difference, (a, b)| difference | (a ^ b))
            == 0;
    if !valid {
        return fail(
            StatusCode::UNAUTHORIZED,
            "The request has no valid bearer token. Send `Authorization: Bearer <token>` with the token from control_token_env or control_token_file.",
        )
        .into_response();
    }
    next.run(request).await
}

async fn health(State(api): State<ControlApi>) -> Result<(StatusCode, Json<Health>), Failure> {
    check_health(&api, false).await
}

async fn ready(State(api): State<ControlApi>) -> Result<(StatusCode, Json<Health>), Failure> {
    check_health(&api, true).await
}

/// Liveness, or with `ready` also readiness; 503 when the check fails.
async fn check_health(
    api: &ControlApi,
    ready: bool,
) -> Result<(StatusCode, Json<Health>), Failure> {
    let status =
        read_status(&api.state_dir).map_err(|e| fail(StatusCode::SERVICE_UNAVAILABLE, e))?;
    let live = status.is_live();
    let is_ready = ready && live && status.state == crate::State::Ready && {
        // Read the settings again: a reload can change api_port.
        let config = settings::load(api.config_path.as_deref(), &api.flags)
            .map_err(|e| fail(StatusCode::INTERNAL_SERVER_ERROR, e))?;
        api_handshake(config.api_port).await.is_ok()
    };
    let passed = if ready { is_ready } else { live };
    let code = if passed {
        StatusCode::OK
    } else {
        StatusCode::SERVICE_UNAVAILABLE
    };
    let health = Health {
        live,
        ready: is_ready,
        state: status.state,
        controller_pid: status.controller_pid,
    };
    Ok((code, Json(health)))
}

async fn openapi() -> Response {
    static DOCUMENT: std::sync::LazyLock<String> =
        std::sync::LazyLock::new(|| crate::openapi::document().to_string());
    (
        [(header::CONTENT_TYPE, "application/json")],
        DOCUMENT.as_str(),
    )
        .into_response()
}

async fn metrics(State(api): State<ControlApi>) -> Response {
    let status = read_status(&api.state_dir).ok();
    (
        [(
            header::CONTENT_TYPE,
            "text/plain; version=0.0.4; charset=utf-8",
        )],
        api.metrics.render(status.as_ref()),
    )
        .into_response()
}

async fn status(State(api): State<ControlApi>) -> Result<Json<Status>, Failure> {
    read_status(&api.state_dir)
        .map(Json)
        .map_err(|e| fail(StatusCode::SERVICE_UNAVAILABLE, e))
}

async fn control(
    State(api): State<ControlApi>,
    Json(request): Json<ControlRequest>,
) -> Result<StatusCode, Failure> {
    send_control(&api.state_dir, request.control).map_err(|e| fail(StatusCode::CONFLICT, e))?;
    Ok(StatusCode::ACCEPTED)
}

async fn config(State(api): State<ControlApi>) -> Result<Json<Config>, Failure> {
    settings::load(api.config_path.as_deref(), &api.flags)
        .map(Json)
        .map_err(|e| fail(StatusCode::INTERNAL_SERVER_ERROR, e))
}

/// Keys that the API never sets: they choose the programs that run, where
/// the controller and its credentials live, and how the API itself is
/// reached. Credentials and the TOTP secret have their own endpoints.
const API_REFUSED: &[&str] = &[
    "executable",
    "agent",
    "settings_dir",
    "control_listen",
    "control_token_env",
    "control_token_file",
    "control_api_full_access",
    "username_env",
    "password_env",
    "totp_secret_env",
    "username_file",
    "password_file",
    "totp_secret_file",
    "vnc_listen",
    "vnc_password_file",
];

/// Risky values that the API sets only with `control_api_full_access`.
fn needs_full_access(key: &str, value: Option<&toml::Value>) -> bool {
    let is = |wanted: toml::Value| value == Some(&wanted);
    match key {
        "trading_mode" => is(toml::Value::String("live".into())),
        "allow_remote_api" | "bypass_order_precautions" => is(toml::Value::Boolean(true)),
        "read_only_api" => is(toml::Value::Boolean(false)),
        "incoming_connection" => is(toml::Value::String("accept".into())),
        "api_listen" | "api_allowed_sources" => true,
        _ => false,
    }
}

async fn update_config(
    State(api): State<ControlApi>,
    Json(update): Json<ConfigUpdate>,
) -> Result<Json<Config>, Failure> {
    let full_access = settings::load(api.config_path.as_deref(), &api.flags)
        .map_err(|e| fail(StatusCode::INTERNAL_SERVER_ERROR, e))?
        .control_api_full_access;
    for (key, value) in &update {
        if API_REFUSED.contains(&key.as_str()) {
            return Err(fail(
                StatusCode::FORBIDDEN,
                format!(
                    "You tried to set {key} through the API. The API cannot change which programs run, the credential sources or the API itself; set {key} in the config file, a flag or an IBKIT_* variable (use PUT /v1/credentials and PUT /v1/totp for credentials)."
                ),
            ));
        }
        if !full_access && needs_full_access(key, value.as_ref()) {
            return Err(fail(
                StatusCode::FORBIDDEN,
                format!(
                    "You tried to set {key} to {} through the API. Live trading, remote or writable API access, accepted incoming connections, bypassed precautions and the API listener need control_api_full_access = true in the config file, a flag or IBKIT_CONTROL_API_FULL_ACCESS.",
                    value
                        .as_ref()
                        .map_or("null".into(), |value| value.to_string())
                ),
            ));
        }
    }
    apply(&api, update).map(Json)
}

/// Remove every setting saved through the API, so the file, flags and
/// variables apply again, and reload. Saved credential and TOTP files stay;
/// PUT them again or set the file keys in the config file.
async fn clear_config(State(api): State<ControlApi>) -> Result<Json<Config>, Failure> {
    let saved = settings::read_api_settings(&api.state_dir)
        .map_err(|e| fail(StatusCode::INTERNAL_SERVER_ERROR, e))?;
    apply(&api, saved.keys().map(|key| (key.clone(), None)).collect()).map(Json)
}

/// Check `update`, save it with the other API settings, and reload the
/// controller. Nothing is saved when the merged configuration is invalid.
fn apply(api: &ControlApi, update: ConfigUpdate) -> Result<Config, Failure> {
    for key in update.keys() {
        if !settings::config_fields().any(|(name, _, _)| name == key) {
            return Err(fail(
                StatusCode::BAD_REQUEST,
                format!(
                    "You sent the unknown setting {key:?}. Use a key that GET /v1/config returns."
                ),
            ));
        }
        if key == "state_dir" {
            return Err(fail(
                StatusCode::BAD_REQUEST,
                "You cannot change state_dir through the API: the controller lock and recovery journal are in it. Stop the controller and start it with the new state_dir.",
            ));
        }
        if api.flags.contains_key(key) {
            return Err(fail(
                StatusCode::CONFLICT,
                format!(
                    "{key} is set by --{} or IBKIT_{}, which override the API. Remove that flag or variable to manage {key} here.",
                    key.replace('_', "-"),
                    key.to_uppercase()
                ),
            ));
        }
    }
    let internal = |e: anyhow::Error| fail(StatusCode::INTERNAL_SERVER_ERROR, e);
    let mut saved = settings::read_api_settings(&api.state_dir).map_err(internal)?;
    for (key, value) in update {
        match value {
            Some(value) => saved.insert(key, value),
            None => saved.remove(&key),
        };
    }
    let mut config = settings::merge(api.config_path.as_deref(), &saved, &api.flags)
        .map_err(|e| fail(StatusCode::BAD_REQUEST, e))?;
    check_config(&mut config).map_err(|e| fail(StatusCode::BAD_REQUEST, e))?;
    let text = toml::to_string(&saved).map_err(|e| internal(e.into()))?;
    write_private(&api.state_dir.join(settings::API_SETTINGS), text.as_bytes())
        .map_err(internal)?;
    send_control(&api.state_dir, crate::Control::Reload)
        .map_err(|e| fail(StatusCode::CONFLICT, e))?;
    Ok(config)
}

async fn credentials(
    State(api): State<ControlApi>,
    Json(request): Json<CredentialsRequest>,
) -> Result<StatusCode, Failure> {
    if [&request.username, &request.password]
        .iter()
        .any(|value| value.is_empty() || value.contains(['\n', '\r', '\0']))
    {
        return Err(fail(
            StatusCode::BAD_REQUEST,
            "You sent an empty or multiline username or password. Send one nonempty line for each.",
        ));
    }
    let directory = api.state_dir.join("credentials");
    let internal = |e: anyhow::Error| fail(StatusCode::INTERNAL_SERVER_ERROR, e);
    let mut update = ConfigUpdate::new();
    for (name, value) in [
        ("username", &request.username),
        ("password", &request.password),
    ] {
        let path = directory.join(name);
        write_private(&path, value.as_bytes()).map_err(internal)?;
        update.insert(
            format!("{name}_file"),
            Some(toml::Value::String(path.display().to_string())),
        );
    }
    apply(&api, update)?;
    Ok(StatusCode::NO_CONTENT)
}

async fn set_totp(
    State(api): State<ControlApi>,
    Json(request): Json<TotpRequest>,
) -> Result<StatusCode, Failure> {
    crate::totp::decode_secret(&request.secret).map_err(|e| fail(StatusCode::BAD_REQUEST, e))?;
    let path = api.state_dir.join("credentials").join("totp_secret");
    write_private(&path, request.secret.trim().as_bytes())
        .map_err(|e| fail(StatusCode::INTERNAL_SERVER_ERROR, e))?;
    apply(
        &api,
        ConfigUpdate::from([(
            "totp_secret_file".into(),
            Some(toml::Value::String(path.display().to_string())),
        )]),
    )?;
    Ok(StatusCode::NO_CONTENT)
}

/// Return to IB Key approval on the phone. A secret in totp_secret_env
/// still applies.
async fn clear_totp(State(api): State<ControlApi>) -> Result<StatusCode, Failure> {
    apply(
        &api,
        ConfigUpdate::from([("totp_secret_file".into(), None)]),
    )?;
    let _ = fs::remove_file(api.state_dir.join("credentials").join("totp_secret"));
    Ok(StatusCode::NO_CONTENT)
}

async fn events(State(api): State<ControlApi>, upgrade: WebSocketUpgrade) -> Response {
    let receiver = api.events.subscribe();
    upgrade.on_upgrade(|socket| stream(socket, receiver))
}

/// Send each controller event as one JSON text message.
async fn stream(mut socket: WebSocket, mut events: broadcast::Receiver<Event>) {
    loop {
        tokio::select! {
            event = events.recv() => match event {
                Ok(event) => {
                    let Ok(text) = serde_json::to_string(&event) else { continue };
                    if socket.send(Message::Text(text.into())).await.is_err() {
                        break;
                    }
                }
                Err(broadcast::error::RecvError::Lagged(_)) => continue,
                Err(broadcast::error::RecvError::Closed) => break,
            },
            message = socket.recv() => match message {
                Some(Ok(Message::Close(_)) | Err(_)) | None => break,
                Some(Ok(_)) => {}
            },
        }
    }
}
