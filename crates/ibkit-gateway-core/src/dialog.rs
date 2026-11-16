use crate::{Config, ErrorCode, ExistingSession, IncomingConnection};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Default)]
pub struct WindowSnapshot {
    pub title: String,
    pub text: String,
    pub buttons: Vec<String>,
    pub modal: bool,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, schemars::JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Dialog {
    Login,
    Configuration,
    SecondFactor,
    SecondFactorSelection,
    SecurityCode,
    LoginFailed,
    ServerDisconnected,
    TooManyAttempts,
    PasswordNotice,
    AccountTasks,
    PaperWarning,
    UnsupportedVersion,
    ExistingSession,
    Relogin,
    AdvisorWarning,
    ExitSettings,
    ApiUnavailable,
    RestartConfirmation,
    TokenExpired,
    RestartNow,
    MarketData,
    Ssl,
    OrderWarning,
    ViewLogs,
    ExportFilename,
    ExportFinished,
    Unknown,
    IncomingConnection,
    LoginHandoff,
    ExitConfirmation,
    /// Second factor prompt for a Mobile Authenticator (TOTP) code.
    SecondFactorCode,
    /// Second factor prompt for a passkey (WebAuthn) device.
    Passkey,
    /// "Attempt N: connecting to server" progress. Clicking OK cancels login.
    Connecting,
    ApiPortChange,
    NewerVersion,
    LoginError,
    NotAvailable,
    TipOfTheDay,
    ShutdownProgress,
    /// "Your password will expire in N day(s)" (a Password Notice that
    /// still allows login).
    PasswordExpiryWarning,
    /// "Are you sure you want to execute ... ctrl-alt-# hotkey action?",
    /// the confirmation of a reconnect shortcut.
    HotkeyConfirmation,
    /// "Your connection was lost. Would you like to re-login?"
    ConnectionLost,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Action {
    Login,
    Configure,
    /// Type the current TOTP code and press OK.
    EnterTotp,
    /// Select the configured second factor device and press OK.
    SelectDevice,
    /// A known transient window: leave it alone.
    Wait,
    Click(&'static str),
    Fail(ErrorCode),
    ColdRestart,
    Report,
}

impl WindowSnapshot {
    pub fn classify(&self) -> Dialog {
        let text = self.text.to_lowercase();
        let title = self.title.to_lowercase();
        let has = |button: &str| self.buttons.iter().any(|value| value == button);
        if text.contains("accept incoming connection") {
            return Dialog::IncomingConnection;
        }
        if text.contains("are you sure you want to exit?") {
            return Dialog::ExitConfirmation;
        }
        if title.contains("trading login handoff") {
            return Dialog::LoginHandoff;
        }
        // Gateway 10.52 Language.properties: Session_expired_login_again, the
        // weekly token expiry that needs a full login.
        if text.contains("soft token=0 received instead of expected permanent")
            || text.contains("security tokens associated with your login credentials have expired")
        {
            return Dialog::TokenExpired;
        }
        // Confirm_Private_Hotkey and Connection_Lost_Confirm_Relogin_Msg.
        if text.contains("hotkey action?") {
            return Dialog::HotkeyConfirmation;
        }
        if text.contains("your connection was lost. would you like to re-login?") {
            return Dialog::ConnectionLost;
        }
        if text.contains("connection to server failed") {
            return Dialog::ServerDisconnected;
        }
        if title.contains("security code card") || title == "enter security code" {
            return Dialog::SecurityCode;
        }
        if title.contains("second factor authentication") {
            return if text.contains("select second factor device") {
                Dialog::SecondFactorSelection
            } else if text.contains("passkey") {
                Dialog::Passkey
            } else if text.contains("mobile authenticator") {
                Dialog::SecondFactorCode
            } else {
                Dialog::SecondFactor
            };
        }
        if (text.contains("connecting to server") || text.contains("trying for another"))
            && !has("IB API")
        {
            return Dialog::Connecting;
        }
        if text.contains("apply the new socket port setting") {
            return Dialog::ApiPortChange;
        }
        if title.contains("newer version") {
            return Dialog::NewerVersion;
        }
        if title == "login error" {
            return Dialog::LoginError;
        }
        if text.contains("not currently available") {
            return Dialog::NotAvailable;
        }
        if title.contains("tip of the day") {
            return Dialog::TipOfTheDay;
        }
        if title.contains("shutdown progress") {
            return Dialog::ShutdownProgress;
        }
        if title.contains(" configuration") {
            return Dialog::Configuration;
        }
        if has("Accept and Continue") {
            return Dialog::OrderWarning;
        }
        if title == "login messages" {
            return Dialog::AccountTasks;
        }
        if title.contains("existing session detected") {
            return Dialog::ExistingSession;
        }
        if title == "re-login is required" {
            return Dialog::Relogin;
        }
        if title.contains("financial advisor warning") {
            return Dialog::AdvisorWarning;
        }
        if title.contains("password notice") {
            // Password_to_expire; Password_expired and lock-out notices
            // share the title and still fail the login.
            return if text.contains("will expire in") {
                Dialog::PasswordExpiryWarning
            } else {
                Dialog::PasswordNotice
            };
        }
        if title.contains("exit session setting") {
            return Dialog::ExitSettings;
        }
        if title.contains("use ssl encryption") {
            return Dialog::Ssl;
        }
        if title.contains("view logs") {
            return Dialog::ViewLogs;
        }
        if title.contains("enter export filename") {
            return Dialog::ExportFilename;
        }
        if title == "login failed" || title == "unrecognized username or password" {
            return Dialog::LoginFailed;
        }
        for (needle, kind) in [
            ("too many failed login attempts", Dialog::TooManyAttempts),
            ("api support is not available", Dialog::ApiUnavailable),
            ("is no longer supported", Dialog::UnsupportedVersion),
            ("minimum supported version", Dialog::UnsupportedVersion),
            ("will be desupported on", Dialog::UnsupportedVersion),
            ("this is not a brokerage account", Dialog::PaperWarning),
            (
                "you have elected to have your trading platform restart automatically",
                Dialog::RestartConfirmation,
            ),
            ("would you like to restart now?", Dialog::RestartNow),
            ("bid, ask and last size display update", Dialog::MarketData),
            ("finished exporting logs", Dialog::ExportFinished),
        ] {
            if text.contains(needle) {
                return kind;
            }
        }
        if has("IB API") && (has("Log In") || has("Paper Log In")) {
            return Dialog::Login;
        }
        Dialog::Unknown
    }
}

impl Dialog {
    /// The login screen or a second-factor prompt.
    pub fn is_authentication(self) -> bool {
        matches!(
            self,
            Self::Login
                | Self::SecondFactor
                | Self::SecondFactorSelection
                | Self::SecondFactorCode
                | Self::Passkey
        )
    }

    pub fn action(self, config: &Config) -> Action {
        match self {
            Self::Login => Action::Login,
            Self::Configuration => Action::Configure,
            // IB Key: the approval happens on the phone.
            Self::SecondFactor => Action::Wait,
            Self::SecondFactorCode => Action::EnterTotp,
            Self::SecondFactorSelection => Action::SelectDevice,
            Self::Passkey if config.passkey_authenticate => Action::Click("Authenticate"),
            Self::Passkey => Action::Fail(ErrorCode::SecurityDialogDetected),
            Self::Connecting | Self::ShutdownProgress => Action::Wait,
            Self::ApiPortChange => Action::Click("Yes"),
            Self::NewerVersion => Action::Click("OK"),
            Self::TipOfTheDay => Action::Click("Close"),
            Self::LoginError | Self::NotAvailable => Action::ColdRestart,
            Self::SecurityCode => Action::Fail(ErrorCode::SecurityDialogDetected),
            Self::LoginFailed | Self::TooManyAttempts | Self::PasswordNotice => {
                Action::Fail(ErrorCode::LoginFailed)
            }
            Self::AccountTasks => Action::Fail(ErrorCode::LoginFailedAccountTasksRequired),
            Self::ApiUnavailable => Action::Fail(ErrorCode::ApiSupportNotAvailable),
            Self::UnsupportedVersion => Action::Fail(ErrorCode::UnsupportedVersion),
            Self::ExistingSession if config.existing_session == ExistingSession::Takeover => {
                Action::Click("OK")
            }
            Self::ExistingSession => Action::Fail(ErrorCode::ExistingSessionDetected),
            Self::LoginHandoff if config.existing_session == ExistingSession::Takeover => {
                Action::Click("Disconnect Other Session")
            }
            Self::LoginHandoff => Action::Fail(ErrorCode::ExistingSessionDetected),
            Self::IncomingConnection => match config.incoming_connection {
                IncomingConnection::Manual => Action::Report,
                IncomingConnection::Accept => Action::Click("OK"),
                IncomingConnection::Reject => Action::Click("No"),
            },
            Self::TokenExpired | Self::ServerDisconnected => Action::ColdRestart,
            Self::PaperWarning => Action::Click("I understand and accept"),
            // Re-login takes the session back from whoever logged in
            // elsewhere; that is a takeover.
            Self::Relogin if config.existing_session == ExistingSession::Takeover => {
                Action::Click("Re-login")
            }
            Self::Relogin => Action::Fail(ErrorCode::ExistingSessionDetected),
            Self::PasswordExpiryWarning if config.dismiss_password_expiry_warning => {
                Action::Click("OK")
            }
            Self::PasswordExpiryWarning => Action::Fail(ErrorCode::LoginFailed),
            // The agent clicks it only right after its own shortcut.
            Self::HotkeyConfirmation => Action::Click("OK"),
            Self::ConnectionLost => Action::ColdRestart,
            Self::AdvisorWarning if config.accept_order_warnings => Action::Click("Yes"),
            Self::OrderWarning if config.accept_order_warnings => {
                Action::Click("Accept and Continue")
            }
            Self::MarketData => Action::Click("I understand - display market data"),
            Self::Ssl => Action::Click("Reconnect using SSL"),
            Self::RestartNow => Action::Click("No"),
            Self::ExitSettings | Self::RestartConfirmation | Self::ExportFinished => {
                Action::Click("OK")
            }
            Self::ViewLogs => Action::Click("Export Today Logs..."),
            Self::ExportFilename => Action::Click("Open"),
            _ => Action::Report,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn window(title: &str, text: &str, buttons: &[&str]) -> WindowSnapshot {
        WindowSnapshot {
            title: title.into(),
            text: text.into(),
            buttons: buttons.iter().map(|b| (*b).into()).collect(),
            modal: true,
        }
    }

    // Texts from Gateway 10.52 Language.properties.
    #[test]
    fn gateway_notices_classify_and_follow_the_policy() {
        let config = Config::default();
        let expiry = window(
            "Password Notice",
            "<html>Your password will expire in <b>3</b> day(s).<br><br>You may change your password at any time",
            &["OK"],
        );
        assert_eq!(expiry.classify(), Dialog::PasswordExpiryWarning);
        assert_eq!(expiry.classify().action(&config), Action::Click("OK"));
        let strict = Config {
            dismiss_password_expiry_warning: false,
            ..Config::default()
        };
        assert_eq!(
            expiry.classify().action(&strict),
            Action::Fail(ErrorCode::LoginFailed)
        );
        let expired = window(
            "Password Notice",
            "<html>Your password has expired and must be reset.",
            &["OK"],
        );
        assert_eq!(expired.classify(), Dialog::PasswordNotice);
        let tokens = window(
            "IBKR Gateway",
            "<html>The security tokens associated with your login credentials have expired<br>(routinely)",
            &["OK"],
        );
        assert_eq!(tokens.classify(), Dialog::TokenExpired);
        let hotkey = window(
            "IBKR Gateway",
            "Are you sure you want to execute \"simulate reconnect\" ctrl-alt-# hotkey action?",
            &["OK", "Cancel"],
        );
        assert_eq!(hotkey.classify(), Dialog::HotkeyConfirmation);
        let lost = window(
            "IBKR Gateway",
            "Your connection was lost. Would you like to re-login?",
            &["Yes", "No"],
        );
        assert_eq!(lost.classify().action(&config), Action::ColdRestart);
        let factor = window(
            "Second Factor Authentication ",
            "Select second factor device",
            &["OK"],
        );
        assert_eq!(factor.classify(), Dialog::SecondFactorSelection);
    }

    #[test]
    fn relogin_takes_over_only_with_the_takeover_policy() {
        let relogin = window(
            "Re-login is required",
            "",
            &["Re-login", "Exit Application"],
        );
        assert_eq!(
            relogin.classify().action(&Config::default()),
            Action::Fail(ErrorCode::ExistingSessionDetected)
        );
        let takeover = Config {
            existing_session: ExistingSession::Takeover,
            ..Config::default()
        };
        assert_eq!(
            relogin.classify().action(&takeover),
            Action::Click("Re-login")
        );
    }
}
