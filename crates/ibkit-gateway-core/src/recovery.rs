use crate::{Config, ErrorCode, schedule::is_within_scheduled_server_reset_times};
use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, schemars::JsonSchema, Serialize)]
#[serde(tag = "phase", rename_all = "snake_case", deny_unknown_fields)]
pub enum RecoveryState {
    Idle,
    Running,
    AwaitingAgent {
        deadline: DateTime<Utc>,
    },
    Waiting {
        retry_at: DateTime<Utc>,
        reason: ErrorCode,
    },
    Exhausted,
    Blocked {
        reason: ErrorCode,
    },
}

/// One controller owns the retry budget across cold starts. A new JVM does not
/// reset this budget or the back-off. A full hour without a recovery request
/// resets both.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Recovery {
    pub state: RecoveryState,
    attempts: u32,
    last_failure: Option<DateTime<Utc>>,
    /// Consecutive logins that ended without Ready for a fault where more
    /// logins can lock the account (`max_login_failures`).
    login_failures: u32,
}

impl Default for Recovery {
    fn default() -> Self {
        Self {
            state: RecoveryState::Idle,
            attempts: 0,
            last_failure: None,
            login_failures: 0,
        }
    }
}

impl Recovery {
    pub fn validate(&self) -> anyhow::Result<()> {
        if (self.attempts == 0) != self.last_failure.is_none()
            || matches!(
                self.state,
                RecoveryState::Waiting { .. } | RecoveryState::Exhausted
            ) && self.attempts == 0
            || matches!(self.state, RecoveryState::Waiting { reason, .. } if !Self::is_retryable(reason))
            || matches!(self.state, RecoveryState::Waiting { retry_at, .. } if self.last_failure.is_some_and(|last| !(1..=86400).contains(&(retry_at - last).num_seconds())))
            || matches!(self.state, RecoveryState::AwaitingAgent { deadline } if deadline > Utc::now() + Duration::days(1))
            || matches!(
                self.state,
                RecoveryState::Blocked {
                    reason: ErrorCode::None
                }
            )
        {
            anyhow::bail!(
                "The saved recovery policy is invalid. Keep its original file for review, stop the controller, and use reset-recovery only after you correct the failure."
            );
        }
        Ok(())
    }

    /// Faults that a new login can repair. Never retried: rejected
    /// credentials (another login risks a lock-out), account tasks, an
    /// unsupported second factor, accounts without API access, unsupported
    /// Gateway versions, and installation or configuration faults.
    pub const fn is_retryable(reason: ErrorCode) -> bool {
        matches!(
            reason,
            ErrorCode::AuthenticationRequired
                | ErrorCode::ConnectionLost
                | ErrorCode::ApiConnectionUnavailable
                | ErrorCode::AgentDisconnected
                | ErrorCode::SoftRestartTimeout
                | ErrorCode::AuthenticationNotAnswered
                | ErrorCode::TwoFactorConfirmationTimeout
                | ErrorCode::ExistingSessionDetected
                | ErrorCode::InitializationTimeout
                | ErrorCode::UnknownMessageWindowDetected
                | ErrorCode::RestartedProcessNotFound
                | ErrorCode::JavaException
                | ErrorCode::ProcessStartFailed
                | ErrorCode::LogExportTimeout
        )
    }

    /// Shortest wait before a retry for `reason`, in seconds: a held IBKR
    /// session needs time to drain, a missed phone approval should not
    /// cause a new request at once, and a refused existing session only
    /// clears when the other session ends.
    pub const fn minimum_delay(reason: ErrorCode) -> u64 {
        match reason {
            ErrorCode::AuthenticationNotAnswered => 1200,
            ErrorCode::TwoFactorConfirmationTimeout => 300,
            ErrorCode::ExistingSessionDetected => 3600,
            _ => 0,
        }
    }

    pub fn attempts(&self) -> u32 {
        self.attempts
    }

    /// Faults after which another login can count toward an IBKR lock-out:
    /// a login dialog that the agent does not know (it can be a rejected
    /// password), and second-factor requests that nobody answered.
    pub const fn risks_lockout(reason: ErrorCode) -> bool {
        matches!(
            reason,
            ErrorCode::UnknownMessageWindowDetected
                | ErrorCode::TwoFactorConfirmationTimeout
                | ErrorCode::AuthenticationNotAnswered
        )
    }

    /// Gateway reached Ready: the count of failed logins starts again.
    /// Returns whether it changed (the caller saves the policy).
    pub fn ready(&mut self) -> bool {
        std::mem::take(&mut self.login_failures) != 0
    }

    pub fn budget_exhausted(&self, config: &Config, now: DateTime<Utc>) -> bool {
        config.recovery_attempts != 0
            && self.attempts >= config.recovery_attempts
            && self
                .last_failure
                .is_none_or(|last| now - last < Duration::hours(1))
    }

    pub fn schedule(
        &mut self,
        config: &Config,
        reason: ErrorCode,
        now: DateTime<Utc>,
    ) -> Option<(u32, DateTime<Utc>)> {
        if matches!(self.state, RecoveryState::Blocked { .. }) {
            return None;
        }
        if reason != ErrorCode::None && (!Self::is_retryable(reason) || !config.auto_recover) {
            self.state = RecoveryState::Blocked { reason };
            return None;
        }
        if matches!(self.state, RecoveryState::Waiting { .. }) {
            return None;
        }
        if !config.auto_recover || reason == ErrorCode::None {
            return None;
        }
        if Self::risks_lockout(reason) {
            self.login_failures = self.login_failures.saturating_add(1);
            if config.max_login_failures != 0 && self.login_failures >= config.max_login_failures {
                self.state = RecoveryState::Blocked {
                    reason: ErrorCode::LoginRetriesExhausted,
                };
                return None;
            }
        }
        if self.budget_exhausted(config, now) {
            self.state = RecoveryState::Exhausted;
            return None;
        }
        if self
            .last_failure
            .is_some_and(|last| now - last >= Duration::hours(1))
        {
            self.attempts = 0;
        }
        self.attempts = self.attempts.saturating_add(1);
        self.last_failure = Some(now);
        let delay = config
            .recovery_delay_secs
            .saturating_mul(1 << (self.attempts - 1).min(20))
            .min(config.recovery_max_delay_secs)
            .max(Self::minimum_delay(reason));
        let retry_at = now + Duration::seconds(delay as i64);
        self.state = RecoveryState::Waiting { retry_at, reason };
        Some((self.attempts, retry_at))
    }

    pub fn can_retry(&self, config: &Config, now: DateTime<Utc>) -> bool {
        matches!(self.state, RecoveryState::Waiting { retry_at, .. } if now >= retry_at)
            && !is_within_scheduled_server_reset_times(now, config.region)
    }

    pub fn retry_started(&mut self) {
        self.state = RecoveryState::Running;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lockout_risks_stop_after_the_limit_and_ready_resets_the_count() {
        let config = Config {
            max_login_failures: 2,
            ..Config::default()
        };
        // A Monday afternoon, outside every reset window.
        let now = DateTime::parse_from_rfc3339("2026-11-16T15:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        let mut recovery = Recovery::default();
        assert!(
            recovery
                .schedule(&config, ErrorCode::TwoFactorConfirmationTimeout, now)
                .is_some()
        );
        recovery.retry_started();
        assert!(recovery.ready(), "Ready resets the count");
        assert!(
            recovery
                .schedule(&config, ErrorCode::TwoFactorConfirmationTimeout, now)
                .is_some()
        );
        recovery.retry_started();
        // A transient fault does not count.
        assert!(
            recovery
                .schedule(&config, ErrorCode::ConnectionLost, now)
                .is_some()
        );
        recovery.retry_started();
        assert!(
            recovery
                .schedule(&config, ErrorCode::UnknownMessageWindowDetected, now)
                .is_none()
        );
        assert_eq!(
            recovery.state,
            RecoveryState::Blocked {
                reason: ErrorCode::LoginRetriesExhausted
            }
        );
    }
}
