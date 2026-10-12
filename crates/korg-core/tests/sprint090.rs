//! Sprint 090 — the full work-item list can be read newest first.
//!
//! WI 3763. The Work Items page walks `GET /api/work-items` up to a 2000-row
//! bound (D-2), and the read was ascending only — so once the corpus passed
//! 2000 (2014 on the 2026-10-12 nightly, 1866 of them closed) the rows the
//! bound cut were the *newest*, while the page spent its budget on closed rows
//! it hides by default. `newest_first` lets the walk spend the bound on the
//! rows that matter and leave the oldest behind Load all.

use korg_core::repo::{create_work_item, list_work_items, PageQuery, WorkItemQuery};
use korg_test_support::{fresh_korg, new, test_project};

#[tokio::test]
async fn newest_first_reverses_the_order_and_pages_from_the_top() {
    let (_c, pool) = fresh_korg().await;
    test_project(&pool).await;
    let mut made = Vec::new();
    for t in ["a", "b", "c"] {
        made.push(
            create_work_item(&pool, new::work_item(t))
                .await
                .unwrap()
                .wi_number,
        );
    }

    let asc = list_work_items(&pool, WorkItemQuery::default())
        .await
        .unwrap();
    let asc: Vec<i64> = asc.items.iter().map(|r| r.wi_number).collect();
    assert_eq!(asc, made, "the default stays ascending");

    let page = list_work_items(
        &pool,
        WorkItemQuery {
            newest_first: true,
            page: PageQuery {
                limit: Some(2),
                offset: Some(0),
            },
            ..Default::default()
        },
    )
    .await
    .unwrap();
    let top: Vec<i64> = page.items.iter().map(|r| r.wi_number).collect();
    assert_eq!(
        top,
        vec![made[2], made[1]],
        "a bounded read keeps the newest"
    );
    assert_eq!(page.total, 3, "the order does not change what is counted");
}
