"""Generated from crates/ibkit-gateway/openapi.json by scripts/gen_gateway_types.py. Do not edit by hand."""

from __future__ import annotations

from enum import Enum
from typing import Any, Literal, TypedDict, TypeVar


class Health(TypedDict):
    """`GET /v1/health` and `GET /v1/health/ready` (no token needed)."""

    live: bool
    ready: bool
    state: State
    controller_pid: int


class State(str, Enum):
    """Controller and Gateway state. `failed`: Gateway stopped on a fault in
    one session, and recovery may follow. `halted`: Gateway is stopped after
    a fault that automatic recovery must not retry; the controller stays up
    and waits for restart, reload or stop. `paused`: an operator (the `pause`
    command or `closedown_at`) stopped Gateway; the controller stays up,
    also across its own restarts, until `restart`.
    """

    STARTING = 'starting'
    LOGGING_IN = 'logging_in'
    AWAITING_SECOND_FACTOR = 'awaiting_second_factor'
    CONFIGURING = 'configuring'
    READY = 'ready'
    RESTARTING = 'restarting'
    STOPPING = 'stopping'
    EXITED = 'exited'
    FAILED = 'failed'
    RECOVERING = 'recovering'
    HALTED = 'halted'
    PAUSED = 'paused'


class Status(TypedDict):
    """Controller status, saved to `state_dir/status.json` every 500 ms and
    returned by `GET /v1/status`.
    """

    session: str
    controller_pid: int
    at: str
    state: State
    agents: list[AgentStatus]
    last_start_result: StartResult
    recovery: RecoveryState | None
    process_group: int | None


class AgentStatus(TypedDict):
    session: str
    pid: int
    at: str
    state: State
    result: StartResult
    max_heap_mb: int | None


class StartResult(TypedDict):
    error_code: ErrorCode
    error_message: str


class ErrorCode(str, Enum):
    """Values 0..=15 match QuantConnect IBAutomater's ErrorCode values."""

    NONE = 'None'
    PROCESS_START_FAILED = 'ProcessStartFailed'
    IB_GATEWAY_VERSION_NOT_INSTALLED = 'IbGatewayVersionNotInstalled'
    JAVA_NOT_FOUND = 'JavaNotFound'
    JAVA_EXCEPTION = 'JavaException'
    LOGIN_FAILED = 'LoginFailed'
    EXISTING_SESSION_DETECTED = 'ExistingSessionDetected'
    SECURITY_DIALOG_DETECTED = 'SecurityDialogDetected'
    TWO_FACTOR_CONFIRMATION_TIMEOUT = 'TwoFactorConfirmationTimeout'
    INITIALIZATION_TIMEOUT = 'InitializationTimeout'
    UNSUPPORTED_VERSION = 'UnsupportedVersion'
    API_SUPPORT_NOT_AVAILABLE = 'ApiSupportNotAvailable'
    RESTARTED_PROCESS_NOT_FOUND = 'RestartedProcessNotFound'
    UNKNOWN_MESSAGE_WINDOW_DETECTED = 'UnknownMessageWindowDetected'
    SOFT_RESTART_TIMEOUT = 'SoftRestartTimeout'
    LOGIN_FAILED_ACCOUNT_TASKS_REQUIRED = 'LoginFailedAccountTasksRequired'
    INVALID_CONFIGURATION = 'InvalidConfiguration'
    AGENT_DISCONNECTED = 'AgentDisconnected'
    AUTHENTICATION_REQUIRED = 'AuthenticationRequired'
    CONNECTION_LOST = 'ConnectionLost'
    API_CONNECTION_UNAVAILABLE = 'ApiConnectionUnavailable'
    LOG_EXPORT_TIMEOUT = 'LogExportTimeout'
    AUTHENTICATION_NOT_ANSWERED = 'AuthenticationNotAnswered'
    TOTP_REJECTED = 'TotpRejected'
    LOGIN_RETRIES_EXHAUSTED = 'LoginRetriesExhausted'


class RecoveryStateIdle(TypedDict):
    phase: Literal['idle']


class RecoveryStateRunning(TypedDict):
    phase: Literal['running']


class RecoveryStateAwaitingAgent(TypedDict):
    phase: Literal['awaiting_agent']
    deadline: str


class RecoveryStateWaiting(TypedDict):
    phase: Literal['waiting']
    retry_at: str
    reason: ErrorCode


class RecoveryStateExhausted(TypedDict):
    phase: Literal['exhausted']


class RecoveryStateBlocked(TypedDict):
    phase: Literal['blocked']
    reason: ErrorCode


RecoveryState = (
    RecoveryStateIdle
    | RecoveryStateRunning
    | RecoveryStateAwaitingAgent
    | RecoveryStateWaiting
    | RecoveryStateExhausted
    | RecoveryStateBlocked
)


class Config(TypedDict):
    executable: str
    agent: str
    state_dir: str
    settings_dir: str
    trading_mode: TradingMode
    auto_login: bool
    api_port: int
    read_only_api: bool
    api_log: bool
    allow_remote_api: bool
    incoming_connection: IncomingConnection
    api_listen: str | None
    auto_recover: bool
    recovery_attempts: int
    recovery_delay_secs: int
    recovery_max_delay_secs: int
    api_probe_interval_secs: int
    api_failure_timeout_secs: int
    region: Region
    bypass_order_precautions: bool
    accept_order_warnings: bool
    existing_session: ExistingSession
    export_logs: bool
    restart_time: str
    initialization_timeout_secs: int
    soft_restart_timeout_secs: int
    shutdown_timeout_secs: int
    second_factor_timeout_secs: int
    second_factor_attempts: int
    unknown_dialog_timeout_secs: int
    username_env: str
    password_env: str
    username_file: str | None
    password_file: str | None
    totp_secret_env: str
    totp_secret_file: str | None
    passkey_authenticate: bool
    java_heap_mb: int | None
    master_client_id: int | None
    manage_display: bool
    control_listen: str | None
    control_token_env: str
    control_token_file: str | None
    alert_webhook_url: str | None
    dismiss_password_expiry_warning: bool
    max_login_failures: int
    api_allowed_sources: str | None
    control_api_full_access: bool
    vnc_listen: str | None
    vnc_password_file: str | None
    closedown_at: str | None
    cold_restart_at: str | None


class TradingMode(str, Enum):
    LIVE = 'live'
    PAPER = 'paper'


class IncomingConnection(str, Enum):
    MANUAL = 'manual'
    ACCEPT = 'accept'
    REJECT = 'reject'


class Region(str, Enum):
    AMERICA = 'america'
    EUROPE = 'europe'
    ASIA = 'asia'


class ExistingSession(str, Enum):
    REFUSE = 'refuse'
    TAKEOVER = 'takeover'


class _EventBase(TypedDict):
    at: str
    session: str
    pid: int


class EventStarted(_EventBase):
    event: Literal['started']


class EventStateChanged(_EventBase):
    event: Literal['state_changed']
    state: State


class EventOutput(_EventBase):
    event: Literal['output']
    message: str


class EventError(_EventBase):
    event: Literal['error']
    result: StartResult


class EventRestarted(_EventBase):
    event: Literal['restarted']


class EventExited(_EventBase):
    event: Literal['exited']
    code: int | None


class EventDialog(_EventBase):
    event: Literal['dialog']
    kind: Dialog
    title: str


class EventRecoveryScheduled(_EventBase):
    event: Literal['recovery_scheduled']
    attempt: int
    retry_at: str
    reason: ErrorCode


class EventApiUnavailable(_EventBase):
    event: Literal['api_unavailable']


class EventApiRestored(_EventBase):
    event: Literal['api_restored']


class EventHalted(_EventBase):
    """Gateway stopped and the controller waits: for an operator after a
    fault that a new login cannot repair, or for the hourly attempt
    budget (`exhausted`).
    """

    event: Literal['halted']
    reason: ErrorCode
    exhausted: bool
    message: str


Event = (
    EventStarted
    | EventStateChanged
    | EventOutput
    | EventError
    | EventRestarted
    | EventExited
    | EventDialog
    | EventRecoveryScheduled
    | EventApiUnavailable
    | EventApiRestored
    | EventHalted
)


class Dialog(str, Enum):
    LOGIN = 'login'
    CONFIGURATION = 'configuration'
    SECOND_FACTOR = 'second_factor'
    SECOND_FACTOR_SELECTION = 'second_factor_selection'
    SECURITY_CODE = 'security_code'
    LOGIN_FAILED = 'login_failed'
    SERVER_DISCONNECTED = 'server_disconnected'
    TOO_MANY_ATTEMPTS = 'too_many_attempts'
    PASSWORD_NOTICE = 'password_notice'
    ACCOUNT_TASKS = 'account_tasks'
    PAPER_WARNING = 'paper_warning'
    UNSUPPORTED_VERSION = 'unsupported_version'
    EXISTING_SESSION = 'existing_session'
    RELOGIN = 'relogin'
    ADVISOR_WARNING = 'advisor_warning'
    EXIT_SETTINGS = 'exit_settings'
    API_UNAVAILABLE = 'api_unavailable'
    RESTART_CONFIRMATION = 'restart_confirmation'
    TOKEN_EXPIRED = 'token_expired'
    RESTART_NOW = 'restart_now'
    MARKET_DATA = 'market_data'
    SSL = 'ssl'
    ORDER_WARNING = 'order_warning'
    VIEW_LOGS = 'view_logs'
    EXPORT_FILENAME = 'export_filename'
    EXPORT_FINISHED = 'export_finished'
    UNKNOWN = 'unknown'
    INCOMING_CONNECTION = 'incoming_connection'
    LOGIN_HANDOFF = 'login_handoff'
    EXIT_CONFIRMATION = 'exit_confirmation'
    API_PORT_CHANGE = 'api_port_change'
    NEWER_VERSION = 'newer_version'
    LOGIN_ERROR = 'login_error'
    NOT_AVAILABLE = 'not_available'
    TIP_OF_THE_DAY = 'tip_of_the_day'
    SHUTDOWN_PROGRESS = 'shutdown_progress'
    SECOND_FACTOR_CODE = 'second_factor_code'
    PASSKEY = 'passkey'
    CONNECTING = 'connecting'
    PASSWORD_EXPIRY_WARNING = 'password_expiry_warning'
    HOTKEY_CONFIRMATION = 'hotkey_confirmation'
    CONNECTION_LOST = 'connection_lost'


class Alert(TypedDict):
    """The JSON body that the controller POSTs to `alert_webhook_url`."""

    alert: AlertKind
    text: str
    at: str
    session: str
    state: State
    reason: ErrorCode
    message: str


class AlertKind(str, Enum):
    """Why an alert was sent."""

    DOWN = 'down'
    RECOVERED = 'recovered'
    HALTED = 'halted'
    EXHAUSTED = 'exhausted'
    CONTROLLER_FAILED = 'controller_failed'


class ApiError(TypedDict):
    """The body of every error response."""

    error: str


class ControlRequest(TypedDict):
    """`POST /v1/control`."""

    control: Control


class Control(str, Enum):
    STOP = 'stop'
    RESTART = 'restart'
    SOFT_RESTART = 'soft_restart'
    EXPORT_LOGS = 'export_logs'
    RECONNECT_ACCOUNT = 'reconnect_account'
    RECONNECT_DATA = 'reconnect_data'
    RELOAD = 'reload'
    PAUSE = 'pause'


class CredentialsRequest(TypedDict):
    """`PUT /v1/credentials`."""

    username: str
    password: str


class TotpRequest(TypedDict):
    """`PUT /v1/totp`: the base32 Mobile Authenticator secret."""

    secret: str


"""Configuration keys (as GET /v1/config returns them) and new values. null removes a value set through the API, so the file or default applies. state_dir and keys set by flags or IBKIT_* variables are refused."""

ConfigUpdate = dict[str, str | int | bool | None]


# Decoders: turn the enum strings of a decoded JSON value into enum members.


_E = TypeVar('_E', bound=Enum)


def _member(enum: type[_E], value: str) -> _E:
    """The member of `enum` with `value`; a value that a newer controller
    added stays a plain string."""
    try:
        return enum(value)
    except ValueError:
        return value  # type: ignore[return-value]


def _decode_Health(value: Any) -> Health:
    value['state'] = _member(State, value['state'])
    return value


def _decode_Status(value: Any) -> Status:
    value['state'] = _member(State, value['state'])
    value['agents'] = [_decode_AgentStatus(_item) for _item in value['agents']]
    value['last_start_result'] = _decode_StartResult(value['last_start_result'])
    value['recovery'] = None if value['recovery'] is None else _decode_RecoveryState(value['recovery'])
    return value


def _decode_AgentStatus(value: Any) -> AgentStatus:
    value['state'] = _member(State, value['state'])
    value['result'] = _decode_StartResult(value['result'])
    return value


def _decode_StartResult(value: Any) -> StartResult:
    value['error_code'] = _member(ErrorCode, value['error_code'])
    return value


def _decode_RecoveryState(value: Any) -> RecoveryState:
    if value['phase'] == 'waiting':
        value['reason'] = _member(ErrorCode, value['reason'])
    if value['phase'] == 'blocked':
        value['reason'] = _member(ErrorCode, value['reason'])
    return value


def _decode_Config(value: Any) -> Config:
    value['trading_mode'] = _member(TradingMode, value['trading_mode'])
    value['incoming_connection'] = _member(IncomingConnection, value['incoming_connection'])
    value['region'] = _member(Region, value['region'])
    value['existing_session'] = _member(ExistingSession, value['existing_session'])
    return value


def _decode_Event(value: Any) -> Event:
    if value['event'] == 'state_changed':
        value['state'] = _member(State, value['state'])
    if value['event'] == 'error':
        value['result'] = _decode_StartResult(value['result'])
    if value['event'] == 'dialog':
        value['kind'] = _member(Dialog, value['kind'])
    if value['event'] == 'recovery_scheduled':
        value['reason'] = _member(ErrorCode, value['reason'])
    if value['event'] == 'halted':
        value['reason'] = _member(ErrorCode, value['reason'])
    return value


def _decode_Alert(value: Any) -> Alert:
    value['alert'] = _member(AlertKind, value['alert'])
    value['state'] = _member(State, value['state'])
    value['reason'] = _member(ErrorCode, value['reason'])
    return value


def _decode_ControlRequest(value: Any) -> ControlRequest:
    value['control'] = _member(Control, value['control'])
    return value
