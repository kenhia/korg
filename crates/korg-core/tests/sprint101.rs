//! Sprint 101 — tagged days for kpidash's calendar card (WI 4664, 0039).
//!
//! The store behind `list_tagged_days` / `add_tagged_day` /
//! `remove_tagged_day`: one call to add, one to remove, bounds that match the
//! feed contract, and no silent success. Dates and names are made up.

use korg_core::error::{ErrorClass, ErrorCode};
use korg_core::repo::{
    add_tagged_day, list_tagged_days, remove_tagged_day, NewTaggedDay, RemoveTaggedDay,
};
use korg_test_support::fresh_korg;
use serde_json::json;

fn tag(v: serde_json::Value) -> NewTaggedDay {
    serde_json::from_value(v).expect("NewTaggedDay")
}

fn sel(v: serde_json::Value) -> RemoveTaggedDay {
    serde_json::from_value(v).expect("RemoveTaggedDay")
}

fn code(e: anyhow::Error) -> ErrorCode {
    e.code()
}

#[tokio::test]
async fn add_list_and_remove_round_trip() {
    let (_c, pool) = fresh_korg().await;
    let off = add_tagged_day(&pool, tag(json!({"date": "2026-03-10", "kind": "off"})))
        .await
        .unwrap();
    assert_eq!((off.kind.as_str(), off.label.as_deref()), ("off", None));
    let bday = add_tagged_day(
        &pool,
        tag(
            json!({"date": "2001-03-10", "kind": "special", "label": "Sam's birthday",
                   "short": "Sam", "yearly": true}),
        ),
    )
    .await
    .unwrap();
    assert!(bday.yearly);

    let rows = list_tagged_days(&pool).await.unwrap();
    assert_eq!(
        rows.iter().map(|r| r.id).collect::<Vec<_>>(),
        vec![bday.id, off.id],
        "ordered by date"
    );
    let wire = serde_json::to_value(&rows[1]).unwrap();
    assert_eq!(
        wire["date"], "2026-03-10",
        "dates are YYYY-MM-DD on the wire"
    );

    // Removing by date narrowed to `off` leaves the birthday standing.
    let gone = remove_tagged_day(&pool, sel(json!({"date": "2026-03-10", "kind": "off"})))
        .await
        .unwrap();
    assert_eq!(gone.iter().map(|r| r.id).collect::<Vec<_>>(), vec![off.id]);
    assert_eq!(list_tagged_days(&pool).await.unwrap().len(), 1);

    // An un-narrowed date reaches the yearly row through its month and day.
    let gone = remove_tagged_day(&pool, sel(json!({"date": "2027-03-10"})))
        .await
        .unwrap();
    assert_eq!(gone.iter().map(|r| r.id).collect::<Vec<_>>(), vec![bday.id]);
    assert!(list_tagged_days(&pool).await.unwrap().is_empty());
}

#[tokio::test]
async fn a_yearly_tag_is_not_reached_before_its_first_year() {
    let (_c, pool) = fresh_korg().await;
    add_tagged_day(
        &pool,
        tag(json!({"date": "2030-06-01", "kind": "special", "label": "Later", "yearly": true})),
    )
    .await
    .unwrap();
    let e = remove_tagged_day(&pool, sel(json!({"date": "2029-06-01"})))
        .await
        .unwrap_err();
    assert_eq!(code(e), ErrorCode::NotFound);
}

#[tokio::test]
async fn the_same_tag_twice_is_a_conflict_naming_the_row() {
    let (_c, pool) = fresh_korg().await;
    let first = add_tagged_day(&pool, tag(json!({"date": "2026-03-10", "kind": "off"})))
        .await
        .unwrap();
    let e = add_tagged_day(&pool, tag(json!({"date": "2026-03-10", "kind": "off"})))
        .await
        .unwrap_err();
    assert!(e.to_string().contains(&first.id.to_string()), "{e}");
    assert_eq!(code(e), ErrorCode::Conflict);
}

#[tokio::test]
async fn bounds_and_kind_rules_are_invalid_input() {
    let (_c, pool) = fresh_korg().await;
    for bad in [
        json!({"date": "2026-03-10", "kind": "holiday"}),
        json!({"date": "2026-03-10", "kind": "special"}),
        json!({"date": "2026-03-10", "kind": "off", "label": "Named"}),
        json!({"date": "2026-03-10", "kind": "off", "short": "x"}),
        json!({"date": "2026-03-10", "kind": "special", "label": "x".repeat(32)}),
        json!({"date": "2026-03-10", "kind": "special", "label": "ok", "short": "x".repeat(13)}),
    ] {
        let e = add_tagged_day(&pool, tag(bad.clone())).await.unwrap_err();
        assert_eq!(code(e), ErrorCode::InvalidInput, "{bad}");
    }
    // The bounds are characters, not bytes: 31 non-ASCII characters fit.
    add_tagged_day(
        &pool,
        tag(json!({"date": "2026-03-10", "kind": "special", "label": "é".repeat(31)})),
    )
    .await
    .unwrap();
}

#[tokio::test]
async fn removing_nothing_or_ambiguously_is_refused() {
    let (_c, pool) = fresh_korg().await;
    for (bad, want) in [
        (json!({"id": 999}), ErrorCode::NotFound),
        (json!({"date": "2026-03-10"}), ErrorCode::NotFound),
        (json!({}), ErrorCode::InvalidInput),
        (
            json!({"id": 1, "date": "2026-03-10"}),
            ErrorCode::InvalidInput,
        ),
        (json!({"id": 1, "kind": "off"}), ErrorCode::InvalidInput),
    ] {
        let e = remove_tagged_day(&pool, sel(bad.clone()))
            .await
            .unwrap_err();
        assert_eq!(code(e), want, "{bad}");
    }
}
