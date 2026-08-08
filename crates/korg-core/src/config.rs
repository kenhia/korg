//! Required IANA timezone configuration and a deterministic local-date clock.
//!
//! The daily-planning feature that motivated this was removed in sprint 050
//! (WI #965); the timezone plumbing stays because "what is today in Ken's
//! timezone" is a question korg keeps needing (#581/#950 are next in line),
//! and the DST edge cases in `tests/config.rs` are exactly the part worth not
//! rebuilding.

use anyhow::{Context, Result};
use jiff::{tz::TimeZone, Timestamp};
use time::macros::format_description;
use time::{Date, OffsetDateTime};

#[derive(Debug, Clone)]
pub struct KorgConfig {
    timezone_name: String,
    timezone: TimeZone,
    fixed_now: Option<OffsetDateTime>,
}

impl KorgConfig {
    pub fn from_env() -> Result<Self> {
        let name = std::env::var("KORG_TIMEZONE")
            .context("KORG_TIMEZONE is required and must be an IANA timezone name")?;
        Self::new(&name)
    }

    pub fn new(name: &str) -> Result<Self> {
        let timezone =
            TimeZone::get(name).with_context(|| format!("invalid IANA KORG_TIMEZONE '{name}'"))?;
        Ok(Self {
            timezone_name: name.to_owned(),
            timezone,
            fixed_now: None,
        })
    }

    pub fn fixed(name: &str, now: OffsetDateTime) -> Result<Self> {
        let mut config = Self::new(name)?;
        config.fixed_now = Some(now);
        Ok(config)
    }

    pub fn timezone_name(&self) -> &str {
        &self.timezone_name
    }

    /// The current calendar date in the configured timezone.
    pub fn local_today(&self) -> Result<Date> {
        self.local_today_at(self.fixed_now.unwrap_or_else(OffsetDateTime::now_utc))
    }

    pub fn local_today_at(&self, now: OffsetDateTime) -> Result<Date> {
        let timestamp = Timestamp::new(now.unix_timestamp(), now.nanosecond() as i32)
            .context("current instant is outside jiff's supported range")?;
        let local = timestamp.to_zoned(self.timezone.clone()).date().to_string();
        Date::parse(&local, &format_description!("[year]-[month]-[day]"))
            .context("failed to convert local calendar date")
    }
}
