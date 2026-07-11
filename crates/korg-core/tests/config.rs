use korg_core::config::KorgConfig;
use time::macros::{date, datetime};

#[test]
fn validates_iana_timezone_and_derives_local_dates_across_dst() {
    assert!(KorgConfig::new("not/a-zone").is_err());
    assert_eq!(KorgConfig::new("UTC").unwrap().timezone_name(), "UTC");

    let eastern = KorgConfig::new("America/New_York").unwrap();
    assert_eq!(
        eastern
            .lifecycle_context_at(datetime!(2026-03-08 04:30 UTC))
            .unwrap()
            .today,
        date!(2026 - 03 - 07),
        "instant before spring transition is still previous local date",
    );
    assert_eq!(
        eastern
            .lifecycle_context_at(datetime!(2026-03-08 07:30 UTC))
            .unwrap()
            .today,
        date!(2026 - 03 - 08),
    );
    assert_eq!(
        eastern
            .lifecycle_context_at(datetime!(2026-11-01 03:30 UTC))
            .unwrap()
            .today,
        date!(2026 - 10 - 31),
        "instant before fall transition is still previous local date",
    );
    assert_eq!(
        eastern
            .lifecycle_context_at(datetime!(2026-11-01 06:30 UTC))
            .unwrap()
            .today,
        date!(2026 - 11 - 01),
    );
}
