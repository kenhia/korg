//! Required IANA timezone configuration and deterministic lifecycle clock.

use anyhow::{Context, Result};
use jiff::{tz::TimeZone, Timestamp};
use time::macros::format_description;
use time::{Date, OffsetDateTime};

use crate::daily_plan::LifecycleContext;

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

    pub fn lifecycle_context(&self) -> Result<LifecycleContext> {
        self.lifecycle_context_at(self.fixed_now.unwrap_or_else(OffsetDateTime::now_utc))
    }

    pub fn lifecycle_context_at(&self, now: OffsetDateTime) -> Result<LifecycleContext> {
        let timestamp = Timestamp::new(now.unix_timestamp(), now.nanosecond() as i32)
            .context("current instant is outside jiff's supported range")?;
        let local = timestamp.to_zoned(self.timezone.clone()).date().to_string();
        let today = Date::parse(&local, &format_description!("[year]-[month]-[day]"))
            .context("failed to convert local calendar date")?;
        Ok(LifecycleContext { today, now })
    }
}
