//! OpenAPI 3.1 description of the control API. The schemas come from the
//! Rust types, so the document cannot drift from the server. It is served
//! at `GET /v1/openapi.json`, printed by `ibkit gateway openapi`, and kept in
//! `crates/ibkit-gateway/openapi.json` (a test fails when that copy is stale).
use crate::{
    Config, Event,
    api::{Alert, ApiError, ControlRequest, CredentialsRequest, Health, Status, TotpRequest},
};
use schemars::{SchemaGenerator, generate::SchemaSettings};
use serde_json::{Value, json};

/// The control API document.
pub fn document() -> Value {
    let mut settings = SchemaSettings::draft2020_12();
    settings.definitions_path = "/components/schemas".into();
    settings.meta_schema = None;
    settings.contract = schemars::generate::Contract::Serialize;
    let mut generator = SchemaGenerator::new(settings);
    let schema = |value: schemars::Schema| value.to_value();
    let health = schema(generator.subschema_for::<Health>());
    let status = schema(generator.subschema_for::<Status>());
    let config = schema(generator.subschema_for::<Config>());
    let event = schema(generator.subschema_for::<Event>());
    let alert = schema(generator.subschema_for::<Alert>());
    let error = schema(generator.subschema_for::<ApiError>());
    let control = schema(generator.subschema_for::<ControlRequest>());
    let credentials = schema(generator.subschema_for::<CredentialsRequest>());
    let totp = schema(generator.subschema_for::<TotpRequest>());
    let mut schemas = generator.take_definitions(true);
    // The launcher and agent defaults differ per operating system (and are
    // found on the machine); the document must be the same everywhere.
    for key in ["executable", "agent"] {
        if let Some(property) = schemas
            .get_mut("Config")
            .and_then(|config| config.pointer_mut(&format!("/properties/{key}")))
            .and_then(Value::as_object_mut)
        {
            property.remove("default");
        }
    }
    schemas.insert(
        "ConfigUpdate".into(),
        json!({
            "description": "Configuration keys (as GET /v1/config returns them) and new values. null removes a value set through the API, so the file or default applies. state_dir and keys set by flags or IBKIT_* variables are refused.",
            "type": "object",
            "additionalProperties": {"type": ["string", "integer", "boolean", "null"]},
        }),
    );
    let body = |schema: &Value| json!({"required": true, "content": {"application/json": {"schema": schema}}});
    let reply = |description: &str, schema: &Value| json!({"description": description, "content": {"application/json": {"schema": schema}}});
    let failure = |description: &str| reply(description, &error);
    let unauthorized = failure("No valid bearer token.");
    let empty = |description: &str| json!({"description": description});
    let secure = json!([{"bearer": []}]);
    json!({
        "openapi": "3.1.0",
        "info": {
            "title": "ibkit control API",
            "version": env!("CARGO_PKG_VERSION"),
            "description": "Health, status, commands, settings, credentials, metrics and events of one ibkit controller. Commands are queued in order through the same command queue as the CLI; each takes effect within a second. Every path except health and this document needs `Authorization: Bearer <token>`. Terminate TLS in front of the controller when it listens beyond loopback.",
        },
        "components": {
            "schemas": schemas,
            "securitySchemes": {"bearer": {"type": "http", "scheme": "bearer", "description": "The token from control_token_env or control_token_file (16 or more characters)."}},
        },
        "paths": {
            "/v1/health": {"get": {
                "operationId": "health", "summary": "Liveness: the controller runs (a halted controller is live).",
                "responses": {"200": reply("Live.", &health), "503": reply("Not live.", &health)},
            }},
            "/v1/health/ready": {"get": {
                "operationId": "ready", "summary": "Readiness: Gateway is logged in and its API socket answered a handshake.",
                "responses": {"200": reply("Ready.", &health), "503": reply("Not ready.", &health)},
            }},
            "/v1/openapi.json": {"get": {
                "operationId": "openapi", "summary": "This document.",
                "responses": {"200": {"description": "The OpenAPI document.", "content": {"application/json": {}}}},
            }},
            "/v1/status": {"get": {
                "operationId": "status", "summary": "Controller status, agents and recovery phase.", "security": secure,
                "responses": {"200": reply("Status.", &status), "401": unauthorized, "503": failure("The controller has not saved a status.")},
            }},
            "/v1/metrics": {"get": {
                "operationId": "metrics", "summary": "Prometheus text metrics (state, recovery, counters, agent heartbeat and heap).", "security": secure,
                "responses": {"200": {"description": "Prometheus text exposition format 0.0.4.", "content": {"text/plain": {"schema": {"type": "string"}}}}, "401": unauthorized},
            }},
            "/v1/control": {"post": {
                "operationId": "control", "summary": "Queue a command: stop, restart, soft_restart, export_logs, reconnect_account, reconnect_data, reload or pause. A failed command is reported as an output event.", "security": secure,
                "requestBody": body(&control),
                "responses": {"202": empty("Queued."), "401": unauthorized, "409": failure("The controller is not running, or soft_restart before Gateway is ready.")},
            }},
            "/v1/config": {
                "get": {
                    "operationId": "config", "summary": "The effective settings: defaults, file, API settings, flags and IBKIT_* variables.", "security": secure,
                    "responses": {"200": reply("Settings.", &config), "401": unauthorized, "500": failure("The settings cannot be read.")},
                },
                "patch": {
                    "operationId": "updateConfig", "summary": "Save settings and start a new Gateway session with them. Nothing is saved when the result is invalid.", "security": secure,
                    "requestBody": body(&json!({"$ref": "#/components/schemas/ConfigUpdate"})),
                    "responses": {"200": reply("The new settings.", &config), "400": failure("An unknown key, state_dir, or an invalid result."), "401": unauthorized, "403": failure("A key that the API never sets (programs, credential sources, the control API, VNC), or a risky value without control_api_full_access."), "409": failure("A flag or IBKIT_* variable sets the key, or the controller is not running.")},
                },
                "delete": {
                    "operationId": "clearConfig", "summary": "Remove every setting saved through the API, so the file, flags and variables apply again, and reload.", "security": secure,
                    "responses": {"200": reply("The new settings.", &config), "401": unauthorized, "409": failure("The controller is not running.")},
                },
            },
            "/v1/credentials": {"put": {
                "operationId": "setCredentials", "summary": "Save the IBKR username and password as 0600 files in state_dir/credentials and reload.", "security": secure,
                "requestBody": body(&credentials),
                "responses": {"204": empty("Saved."), "400": failure("An empty or multiline value."), "401": unauthorized, "409": failure("A flag sets the credential files, or the controller is not running.")},
            }},
            "/v1/totp": {
                "put": {
                    "operationId": "setTotpSecret", "summary": "Save the base32 Mobile Authenticator secret and reload; codes are then entered at login.", "security": secure,
                    "requestBody": body(&totp),
                    "responses": {"204": empty("Saved."), "400": failure("The secret is not base32."), "401": unauthorized, "409": failure("A flag sets totp_secret_file, or the controller is not running.")},
                },
                "delete": {
                    "operationId": "clearTotpSecret", "summary": "Remove the saved secret and return to IB Key approval on the phone.", "security": secure,
                    "responses": {"204": empty("Removed."), "401": unauthorized, "409": failure("A flag sets totp_secret_file, or the controller is not running.")},
                },
            },
            "/v1/events": {"get": {
                "operationId": "events", "summary": "WebSocket: one JSON text message per controller event from the moment of connection.", "security": secure,
                "description": "Each message is an Event. The `event` field names its kind.",
                "responses": {"101": {"description": "Switching to WebSocket; messages are Event objects.", "content": {"application/json": {"schema": event}}}, "401": unauthorized},
            }},
        },
        "webhooks": {
            "alert": {"post": {
                "operationId": "alert", "summary": "Sent to alert_webhook_url when Gateway goes down, comes back, halts, exhausts its recovery budget, or the controller fails.",
                "requestBody": body(&alert),
                "responses": {"200": empty("Any 2xx status accepts the alert; others are retried after 2, 10 and 30 seconds.")},
            }},
        },
    })
}
