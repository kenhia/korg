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
    frame_ancestors: Vec<String>,
}

impl KorgConfig {
    pub fn from_env() -> Result<Self> {
        let name = std::env::var("KORG_TIMEZONE")
            .context("KORG_TIMEZONE is required and must be an IANA timezone name")?;
        let mut config = Self::new(&name)?;
        config.frame_ancestors =
            parse_origins(&std::env::var("KORG_FRAME_ANCESTORS").unwrap_or_default());
        Ok(config)
    }

    pub fn new(name: &str) -> Result<Self> {
        let timezone =
            TimeZone::get(name).with_context(|| format!("invalid IANA KORG_TIMEZONE '{name}'"))?;
        Ok(Self {
            timezone_name: name.to_owned(),
            timezone,
            fixed_now: None,
            // Default closed: a korg that was told nothing lets nobody frame it.
            frame_ancestors: Vec::new(),
        })
    }

    pub fn fixed(name: &str, now: OffsetDateTime) -> Result<Self> {
        let mut config = Self::new(name)?;
        config.fixed_now = Some(now);
        Ok(config)
    }

    /// Build a config with an explicit embed allowlist, for tests and callers
    /// that are not reading the environment.
    pub fn with_frame_ancestors(mut self, origins: &str) -> Self {
        self.frame_ancestors = parse_origins(origins);
        self
    }

    pub fn timezone_name(&self) -> &str {
        &self.timezone_name
    }

    /// The origins permitted to embed korg in an iframe (WI #1468) — empty
    /// unless `KORG_FRAME_ANCESTORS` says otherwise.
    pub fn frame_ancestors(&self) -> &[String] {
        &self.frame_ancestors
    }

    /// The `frame-ancestors` directive to serve, as a complete CSP value.
    ///
    /// **This is not authentication.** It decides which origins a *browser*
    /// will let paint korg inside their page; it changes nothing about who may
    /// read or write korg. The tailnet is the perimeter, and an entry here is a
    /// statement that an origin is allowed to frame korg, not that its users
    /// are allowed anything. Reading this list as an access-control list is the
    /// mistake #1468 was filed to prevent, which is why the sentence is here
    /// rather than only in the docs.
    ///
    /// Unset means `'none'` rather than an omitted header: an absent
    /// `frame-ancestors` means *anyone* may frame korg, so silence and
    /// "nobody" are opposite answers and the default has to be spelled.
    pub fn frame_ancestors_policy(&self) -> String {
        if self.frame_ancestors.is_empty() {
            "frame-ancestors 'none'".to_string()
        } else {
            format!("frame-ancestors {}", self.frame_ancestors.join(" "))
        }
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

/// A comma-separated origin list, trimmed and emptied of blanks.
///
/// Comma-separated to match `KORG_CORS_ORIGINS`, which the operator is already
/// setting beside it; the CSP header itself is space-separated, and
/// [`KorgConfig::frame_ancestors_policy`] does that join. No validation beyond
/// non-emptiness: an origin that is not a valid CSP source is one the browser
/// ignores, and korg refusing to start over a typo in the embed list would be
/// a worse failure than a pane that does not load.
fn parse_origins(raw: &str) -> Vec<String> {
    raw.split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
        .collect()
}
