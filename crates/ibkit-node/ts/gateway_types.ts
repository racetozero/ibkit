// Generated from crates/ibkit-gateway/openapi.json by scripts/gen_gateway_types.py. Do not edit by hand.

/** `GET /v1/health` and `GET /v1/health/ready` (no token needed). */
export interface Health {
  /** The controller runs and saved its status in the last 5 seconds. */
  live: boolean;
  /** Gateway is logged in and its API socket answered a handshake. */
  ready: boolean;
  state: GatewayState;
  controller_pid: number;
}

/**
 * Controller and Gateway state. `failed`: Gateway stopped on a fault in
 * one session, and recovery may follow. `halted`: Gateway is stopped after
 * a fault that automatic recovery must not retry; the controller stays up
 * and waits for restart, reload or stop. `paused`: an operator (the `pause`
 * command or `closedown_at`) stopped Gateway; the controller stays up,
 * also across its own restarts, until `restart`.
 */
export enum GatewayState {
  Starting = 'starting',
  LoggingIn = 'logging_in',
  AwaitingSecondFactor = 'awaiting_second_factor',
  Configuring = 'configuring',
  Ready = 'ready',
  Restarting = 'restarting',
  Stopping = 'stopping',
  Exited = 'exited',
  Failed = 'failed',
  Recovering = 'recovering',
  Halted = 'halted',
  Paused = 'paused',
}

/**
 * Controller status, saved to `state_dir/status.json` every 500 ms and
 * returned by `GET /v1/status`.
 */
export interface Status {
  session: string;
  controller_pid: number;
  at: string;
  state: GatewayState;
  agents: AgentStatus[];
  last_start_result: StartResult;
  /**
   * Automatic recovery phase. With state `halted`, `blocked` waits for an
   * operator and `exhausted` for the hourly attempt budget.
   */
  recovery: RecoveryState | null;
  /**
   * Process group of the Gateway launcher that this controller started.
   * A replacement controller only tests whether the group still exists.
   */
  process_group: number | null;
}

export interface AgentStatus {
  session: string;
  pid: number;
  at: string;
  state: GatewayState;
  result: StartResult;
  /** The JVM's maximum heap (Runtime.maxMemory) in MiB. */
  max_heap_mb: number | null;
}

export interface StartResult {
  error_code: ErrorCode;
  error_message: string;
}

/** Values 0..=15 match QuantConnect IBAutomater's ErrorCode values. */
export enum ErrorCode {
  None = 'None',
  ProcessStartFailed = 'ProcessStartFailed',
  IbGatewayVersionNotInstalled = 'IbGatewayVersionNotInstalled',
  JavaNotFound = 'JavaNotFound',
  JavaException = 'JavaException',
  LoginFailed = 'LoginFailed',
  ExistingSessionDetected = 'ExistingSessionDetected',
  SecurityDialogDetected = 'SecurityDialogDetected',
  TwoFactorConfirmationTimeout = 'TwoFactorConfirmationTimeout',
  InitializationTimeout = 'InitializationTimeout',
  UnsupportedVersion = 'UnsupportedVersion',
  ApiSupportNotAvailable = 'ApiSupportNotAvailable',
  RestartedProcessNotFound = 'RestartedProcessNotFound',
  UnknownMessageWindowDetected = 'UnknownMessageWindowDetected',
  SoftRestartTimeout = 'SoftRestartTimeout',
  LoginFailedAccountTasksRequired = 'LoginFailedAccountTasksRequired',
  InvalidConfiguration = 'InvalidConfiguration',
  AgentDisconnected = 'AgentDisconnected',
  AuthenticationRequired = 'AuthenticationRequired',
  ConnectionLost = 'ConnectionLost',
  ApiConnectionUnavailable = 'ApiConnectionUnavailable',
  LogExportTimeout = 'LogExportTimeout',
  AuthenticationNotAnswered = 'AuthenticationNotAnswered',
  TotpRejected = 'TotpRejected',
  LoginRetriesExhausted = 'LoginRetriesExhausted',
}

export type RecoveryState =
  | { phase: 'idle' }
  | { phase: 'running' }
  | { phase: 'awaiting_agent'; deadline: string }
  | { phase: 'waiting'; retry_at: string; reason: ErrorCode }
  | { phase: 'exhausted' }
  | { phase: 'blocked'; reason: ErrorCode };

export interface GatewayConfig {
  executable: string;
  agent: string;
  state_dir: string;
  settings_dir: string;
  trading_mode: TradingMode;
  auto_login: boolean;
  api_port: number;
  read_only_api: boolean;
  api_log: boolean;
  allow_remote_api: boolean;
  incoming_connection: IncomingConnection;
  api_listen: string | null;
  auto_recover: boolean;
  /** Retry limit for automatic recovery; 0 retries without limit. */
  recovery_attempts: number;
  /**
   * First retry delay; each further retry doubles it up to
   * `recovery_max_delay_secs`.
   */
  recovery_delay_secs: number;
  recovery_max_delay_secs: number;
  api_probe_interval_secs: number;
  api_failure_timeout_secs: number;
  region: Region;
  bypass_order_precautions: boolean;
  accept_order_warnings: boolean;
  existing_session: ExistingSession;
  export_logs: boolean;
  /** 24-hour local system time. The container sets TZ explicitly. */
  restart_time: string;
  initialization_timeout_secs: number;
  soft_restart_timeout_secs: number;
  shutdown_timeout_secs: number;
  second_factor_timeout_secs: number;
  second_factor_attempts: number;
  /**
   * How long an unknown modal dialog may block authentication before
   * the login fails. Unknown dialogs are never clicked.
   */
  unknown_dialog_timeout_secs: number;
  username_env: string;
  password_env: string;
  username_file: string | null;
  password_file: string | null;
  /**
   * Environment variable with the base32 Mobile Authenticator (TOTP)
   * secret. When it is absent and no file is set, second-factor login
   * waits for IB Key approval on the phone.
   */
  totp_secret_env: string;
  totp_secret_file: string | null;
  /**
   * Press Authenticate on a passkey (WebAuthn) prompt. An authenticator
   * beside Gateway must complete it; otherwise a passkey prompt fails.
   */
  passkey_authenticate: boolean;
  /** Maximum Java heap in MiB (Gateway's default is 768). */
  java_heap_mb: number | null;
  /** Master API client ID: the client that receives all orders. */
  master_client_id: number | null;
  manage_display: boolean;
  /** Control API (REST and WebSocket) listener; `None` disables it. */
  control_listen: string | null;
  /** Environment variable with the control API bearer token. */
  control_token_env: string;
  control_token_file: string | null;
  /**
   * HTTP(S) URL that receives a JSON POST when Gateway goes down, comes
   * back, halts for an operator, or the controller fails. The body has
   * a `text` field, so Slack and Mattermost incoming webhooks accept it.
   */
  alert_webhook_url: string | null;
  /**
   * Click OK on "Your password will expire in N day(s)" and report it as
   * an event. Off: the warning fails the login, as before IBC's
   * DismissPasswordExpiryWarning.
   */
  dismiss_password_expiry_warning: boolean;
  /**
   * Consecutive logins that end without Ready, for a fault where another
   * login can lock the account (an unknown login dialog, unanswered or
   * rejected second factor), before retries stop; 0 has no limit.
   */
  max_login_failures: number;
  /**
   * Comma-separated source addresses or CIDR ranges that may connect to
   * `api_listen`, for example `10.0.0.0/8,192.168.1.20`. Absent: any.
   * Behind Docker port publishing the source can be the bridge address.
   */
  api_allowed_sources: string | null;
  /**
   * Let the control API set risky values: live trading, remote API
   * access, accepting incoming connections, a writable API, bypassed
   * order precautions, and `api_listen`. Only the file, flags and
   * variables can turn this on.
   */
  control_api_full_access: boolean;
  /**
   * Serve the virtual display over VNC (x11vnc) on this address, for
   * account tasks and unknown dialogs. Needs `manage_display` and
   * `vnc_password_file`. Keep it on loopback and use a tunnel.
   */
  vnc_listen: string | null;
  vnc_password_file: string | null;
  /**
   * Weekly time (local system time) at which the controller pauses
   * Gateway, for example `Fri 23:00` (IBC ClosedownAt).
   */
  closedown_at: string | null;
  /**
   * Weekly time (local system time) of a cold restart with a new login,
   * for example `Sun 07:05` (IBC ColdRestartTime): it moves the weekly
   * re-authentication to a time when an operator can approve it.
   */
  cold_restart_at: string | null;
}

export enum TradingMode {
  Live = 'live',
  Paper = 'paper',
}

export enum IncomingConnection {
  Manual = 'manual',
  Accept = 'accept',
  Reject = 'reject',
}

export enum Region {
  America = 'america',
  Europe = 'europe',
  Asia = 'asia',
}

export enum ExistingSession {
  Refuse = 'refuse',
  Takeover = 'takeover',
}

export type GatewayEvent = { at: string; session: string; pid: number } & (
  | { event: 'started' }
  | { event: 'state_changed'; state: GatewayState }
  | { event: 'output'; message: string }
  | { event: 'error'; result: StartResult }
  | { event: 'restarted' }
  | { event: 'exited'; code: number | null }
  | { event: 'dialog'; kind: Dialog; title: string }
  | { event: 'recovery_scheduled'; attempt: number; retry_at: string; reason: ErrorCode }
  | { event: 'api_unavailable' }
  | { event: 'api_restored' }
  | { event: 'halted'; reason: ErrorCode; exhausted: boolean; message: string }
);

export enum Dialog {
  Login = 'login',
  Configuration = 'configuration',
  SecondFactor = 'second_factor',
  SecondFactorSelection = 'second_factor_selection',
  SecurityCode = 'security_code',
  LoginFailed = 'login_failed',
  ServerDisconnected = 'server_disconnected',
  TooManyAttempts = 'too_many_attempts',
  PasswordNotice = 'password_notice',
  AccountTasks = 'account_tasks',
  PaperWarning = 'paper_warning',
  UnsupportedVersion = 'unsupported_version',
  ExistingSession = 'existing_session',
  Relogin = 'relogin',
  AdvisorWarning = 'advisor_warning',
  ExitSettings = 'exit_settings',
  ApiUnavailable = 'api_unavailable',
  RestartConfirmation = 'restart_confirmation',
  TokenExpired = 'token_expired',
  RestartNow = 'restart_now',
  MarketData = 'market_data',
  Ssl = 'ssl',
  OrderWarning = 'order_warning',
  ViewLogs = 'view_logs',
  ExportFilename = 'export_filename',
  ExportFinished = 'export_finished',
  Unknown = 'unknown',
  IncomingConnection = 'incoming_connection',
  LoginHandoff = 'login_handoff',
  ExitConfirmation = 'exit_confirmation',
  ApiPortChange = 'api_port_change',
  NewerVersion = 'newer_version',
  LoginError = 'login_error',
  NotAvailable = 'not_available',
  TipOfTheDay = 'tip_of_the_day',
  ShutdownProgress = 'shutdown_progress',
  SecondFactorCode = 'second_factor_code',
  Passkey = 'passkey',
  Connecting = 'connecting',
  PasswordExpiryWarning = 'password_expiry_warning',
  HotkeyConfirmation = 'hotkey_confirmation',
  ConnectionLost = 'connection_lost',
}

/** The JSON body that the controller POSTs to `alert_webhook_url`. */
export interface GatewayAlert {
  alert: AlertKind;
  /** One readable line, for chat webhooks (Slack, Mattermost). */
  text: string;
  at: string;
  session: string;
  state: GatewayState;
  reason: ErrorCode;
  message: string;
}

/** Why an alert was sent. */
export enum AlertKind {
  Down = 'down',
  Recovered = 'recovered',
  Halted = 'halted',
  Exhausted = 'exhausted',
  ControllerFailed = 'controller_failed',
}

/** The body of every error response. */
export interface ApiError {
  error: string;
}

/** `POST /v1/control`. */
export interface ControlRequest {
  control: Control;
}

export enum Control {
  Stop = 'stop',
  Restart = 'restart',
  SoftRestart = 'soft_restart',
  ExportLogs = 'export_logs',
  ReconnectAccount = 'reconnect_account',
  ReconnectData = 'reconnect_data',
  Reload = 'reload',
  Pause = 'pause',
}

/** `PUT /v1/credentials`. */
export interface CredentialsRequest {
  username: string;
  password: string;
}

/** `PUT /v1/totp`: the base32 Mobile Authenticator secret. */
export interface TotpRequest {
  secret: string;
}

/** Configuration keys (as GET /v1/config returns them) and new values. null removes a value set through the API, so the file or default applies. state_dir and keys set by flags or IBKIT_* variables are refused. */
export type ConfigUpdate = Record<string, string | number | boolean | null>;
