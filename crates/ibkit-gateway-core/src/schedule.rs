use chrono::{DateTime, Datelike, Timelike, Utc, Weekday};
use chrono_tz::{America::New_York, Asia::Hong_Kong, Europe::Zurich};

#[derive(
    Clone,
    Copy,
    Debug,
    Default,
    Eq,
    PartialEq,
    schemars::JsonSchema,
    serde::Deserialize,
    serde::Serialize,
)]
#[serde(rename_all = "snake_case")]
pub enum Region {
    #[default]
    America,
    Europe,
    Asia,
}

impl Region {
    pub fn from_peer(peer: &str) -> Self {
        let host = peer.split(':').next().unwrap_or(peer);
        if host.starts_with("zdc1") {
            Self::Europe
        } else if host.starts_with("hdc1") || host.starts_with("mcgw1") {
            Self::Asia
        } else {
            Self::America
        }
    }
}

pub fn is_within_weekend_server_reset_times(at: DateTime<Utc>) -> bool {
    let local = at.with_timezone(&New_York);
    let second = local.num_seconds_from_midnight();
    match local.weekday() {
        Weekday::Fri => second > (22 * 60 + 45) * 60,
        Weekday::Sat => true,
        Weekday::Sun => second < 120 * 60,
        _ => false,
    }
}

pub fn is_within_scheduled_server_reset_times(at: DateTime<Utc>, region: Region) -> bool {
    if is_within_weekend_server_reset_times(at) {
        return true;
    }
    let ny = at.with_timezone(&New_York);
    let second = ny.num_seconds_from_midnight();
    match region {
        Region::America => !(3600..=(23 * 60 + 30) * 60).contains(&second),
        Region::Europe => {
            let eu = at.with_timezone(&Zurich);
            let s = eu.num_seconds_from_midnight();
            s > 330 * 60 && s < 420 * 60
        }
        Region::Asia => {
            let hk = at.with_timezone(&Hong_Kong);
            let s = hk.num_seconds_from_midnight();
            (second > 975 * 60 && second < 1035 * 60) || (s > 1200 * 60 && s < 1275 * 60)
        }
    }
}

/// The first minute after `at` outside the scheduled reset times of
/// `region` (the weekend window is about 27 hours). `at` itself when it is
/// outside them.
pub fn reset_window_end(at: DateTime<Utc>, region: Region) -> DateTime<Utc> {
    let mut end = at;
    // Three days of minutes bound the search; no window is that long.
    for _ in 0..3 * 24 * 60 {
        if !is_within_scheduled_server_reset_times(end, region) {
            return end;
        }
        end += chrono::Duration::minutes(1);
    }
    end
}

/// A weekly local time such as `Sun 07:05` (`closedown_at`,
/// `cold_restart_at`).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Weekly {
    pub day: Weekday,
    pub hour: u32,
    pub minute: u32,
}

impl Weekly {
    pub fn parse(value: &str) -> anyhow::Result<Self> {
        let (day, time) = value
            .trim()
            .split_once(char::is_whitespace)
            .ok_or_else(|| anyhow::anyhow!("no weekday and time"))?;
        let day: Weekday = day
            .parse()
            .map_err(|_| anyhow::anyhow!("unknown weekday {day:?}"))?;
        let (hour, minute) = time
            .trim()
            .split_once(':')
            .ok_or_else(|| anyhow::anyhow!("no HH:MM time"))?;
        let (hour, minute): (u32, u32) = (hour.parse()?, minute.parse()?);
        anyhow::ensure!(hour < 24 && minute < 60, "the time is not HH:MM");
        Ok(Self { day, hour, minute })
    }

    /// Whether the time occurs in `(after, until]` in the local time zone.
    pub fn occurs_between<Tz: chrono::TimeZone>(
        &self,
        after: DateTime<Tz>,
        until: DateTime<Tz>,
    ) -> bool {
        let mut day = after.date_naive();
        while day <= until.date_naive() {
            if day.weekday() == self.day
                && let Some(time) = day.and_hms_opt(self.hour, self.minute, 0)
                && let Some(at) = after.timezone().from_local_datetime(&time).earliest()
                && at > after
                && at <= until
            {
                return true;
            }
            let Some(next) = day.succ_opt() else { break };
            day = next;
        }
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    #[test]
    fn weekly_times_parse_and_occur_once() {
        let weekly = Weekly::parse("Sun 07:05").unwrap();
        assert_eq!(
            (weekly.day, weekly.hour, weekly.minute),
            (Weekday::Sun, 7, 5)
        );
        assert!(Weekly::parse("Funday 07:05").is_err());
        assert!(Weekly::parse("Sun 24:00").is_err());
        // 2026-11-15 is a Sunday.
        let before = Utc.with_ymd_and_hms(2026, 11, 15, 7, 4, 59).unwrap();
        let after = Utc.with_ymd_and_hms(2026, 11, 15, 7, 5, 0).unwrap();
        assert!(weekly.occurs_between(before, after));
        assert!(!weekly.occurs_between(after, after + chrono::Duration::hours(1)));
        assert!(!weekly.occurs_between(before - chrono::Duration::days(1), before));
    }

    #[test]
    fn the_weekend_window_ends_on_sunday_morning() {
        // Saturday 2026-11-14 12:00 New York (17:00 UTC).
        let saturday = Utc.with_ymd_and_hms(2026, 11, 14, 17, 0, 0).unwrap();
        let end = reset_window_end(saturday, Region::Europe);
        assert!(!is_within_scheduled_server_reset_times(end, Region::Europe));
        assert_eq!(end.with_timezone(&New_York).weekday(), Weekday::Sun);
        let monday = Utc.with_ymd_and_hms(2026, 11, 16, 15, 0, 0).unwrap();
        assert_eq!(reset_window_end(monday, Region::America), monday);
    }
}
