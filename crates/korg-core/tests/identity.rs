//! 0009_identity remap test: seed a pre-0009 database with the production
//! shape — work items whose wi_numbers diverge from node ids, non-workitem
//! nodes squatting on wi_number-valued ids, relationships and comments across
//! kinds — then apply 0009 and assert one-number identity with every edge
//! intact. (schema.rs covers the clean full-migrator path; this test raw-applies
//! the SQL files so it can seed between 0008 and 0009.)

use korg_core::repo::create_work_item;
use korg_test_support::{fresh_korg, new, raw_postgres};
use sqlx::{Executor, Row};

const MIGRATIONS: &[(&str, &str)] = &[
    ("0001", include_str!("../migrations/0001_init.sql")),
    ("0002", include_str!("../migrations/0002_link.sql")),
    ("0003", include_str!("../migrations/0003_slot.sql")),
    (
        "0004",
        include_str!("../migrations/0004_link_disposition.sql"),
    ),
    ("0005", include_str!("../migrations/0005_slot_unique.sql")),
    (
        "0006",
        include_str!("../migrations/0006_relationship_unique.sql"),
    ),
    (
        "0007",
        include_str!("../migrations/0007_comment_node_rename.sql"),
    ),
    (
        "0008",
        include_str!("../migrations/0008_sprint_proposal.sql"),
    ),
];
const IDENTITY: &str = include_str!("../migrations/0009_identity.sql");
const SEQUENCE_FIX: &str = include_str!("../migrations/0015_node_sequence_fresh_install.sql");

#[tokio::test]
async fn identity_remap_preserves_edges() {
    // Raw pool: this suite applies the migration files by hand so it can seed
    // the pre-0009 shape between 0008 and 0009.
    let (_pg, pool) = raw_postgres().await;

    for (name, sql) in MIGRATIONS {
        pool.execute(*sql)
            .await
            .unwrap_or_else(|e| panic!("apply {name}: {e}"));
    }

    // Seed the divergent shape:
    //   node 1 = card (squats on wi_number 1)
    //   node 2 = workitem wi_number 1   (misaligned)
    //   node 3 = workitem wi_number 3   (already aligned)
    //   node 4 = workitem wi_number 2   (misaligned; target 2 is node 2's old id)
    //   relationship card <-> wi#1; comment on wi#1; wi#2 parent = wi#1
    pool.execute(
        r#"
        SELECT setval(pg_get_serial_sequence('node','id'), 1, false);
        INSERT INTO node (kind) VALUES ('card'), ('workitem'), ('workitem'), ('workitem');
        INSERT INTO card (node_id, title, status, rank) VALUES (1, 'squatter', 'Backlog', 1);
        INSERT INTO workitem (node_id, wi_number, wi_type, wi_status, wi_tshirt, title, content)
        VALUES (2, 1, 'task', 'open', 'S', 'wi one', 'c'),
               (3, 3, 'task', 'open', 'S', 'wi three', 'c'),
               (4, 2, 'task', 'open', 'S', 'wi two', 'c');
        UPDATE workitem SET parent_node_id = 2 WHERE node_id = 4;
        INSERT INTO relationship (left_id, right_id, relationship) VALUES (1, 2, 'related');
        INSERT INTO comment (node_id, body) VALUES (2, 'note on wi one');
        "#,
    )
    .await
    .expect("seed");

    pool.execute(IDENTITY).await.expect("apply 0009");

    // one number everywhere
    let rows = sqlx::query("SELECT node_id, wi_number FROM workitem")
        .fetch_all(&pool)
        .await
        .expect("select workitem");
    for r in &rows {
        assert_eq!(
            r.get::<i64, _>("node_id"),
            r.get::<i64, _>("wi_number"),
            "node_id must equal wi_number"
        );
    }
    assert_eq!(rows.len(), 3);

    // the squatting card moved off id 1 and kept its detail row
    let card_id: i64 = sqlx::query("SELECT node_id FROM card WHERE title = 'squatter'")
        .fetch_one(&pool)
        .await
        .expect("card")
        .get("node_id");
    assert!(
        card_id > 3,
        "squatter renumbered above the workitems, got {card_id}"
    );

    // edges followed: relationship now card_id <-> 1, comment on node 1, parent of wi#2 is 1
    let rel: (i64, i64) = sqlx::query("SELECT left_id, right_id FROM relationship")
        .fetch_one(&pool)
        .await
        .map(|r| (r.get("left_id"), r.get("right_id")))
        .expect("relationship");
    assert_eq!(rel, (card_id, 1));
    let comment_node: i64 = sqlx::query("SELECT node_id FROM comment")
        .fetch_one(&pool)
        .await
        .expect("comment")
        .get("node_id");
    assert_eq!(comment_node, 1);
    let parent: i64 = sqlx::query("SELECT parent_node_id FROM workitem WHERE wi_number = 2")
        .fetch_one(&pool)
        .await
        .expect("parent")
        .get("parent_node_id");
    assert_eq!(parent, 1);

    // and the future stays unified: a fresh insert gets wi_number == node id
    let new_id: i64 = sqlx::query("INSERT INTO node (kind) VALUES ('workitem') RETURNING id")
        .fetch_one(&pool)
        .await
        .expect("new node")
        .get("id");
    let wi: i64 = sqlx::query(
        "INSERT INTO workitem (node_id, wi_number, wi_type, wi_status, wi_tshirt, title, content) \
         VALUES ($1, $1, 'task', 'open', 'S', 'fresh', 'c') RETURNING wi_number",
    )
    .bind(new_id)
    .fetch_one(&pool)
    .await
    .expect("new wi")
    .get("wi_number");
    assert_eq!(wi, new_id);
}

/// WI #552 — a fresh install can mint node #1.
///
/// 0009's `setval(seq, GREATEST(MAX(id), 1))` consumes id 1 on an empty
/// database, so before 0015 the first node ever created got id 2 and work item
/// #1 was unreachable forever. This drives the *real* migrator (not the
/// hand-applied files above) because the fix is a later migration and the
/// ordering between them is the thing under test.
#[tokio::test]
async fn a_fresh_database_mints_node_one() {
    let (_pg, pool) = fresh_korg().await;

    let wi = create_work_item(&pool, new::work_item("the first work item"))
        .await
        .expect("create wi");

    assert_eq!(
        wi.wi_number, 1,
        "the first work item on a fresh database must be #1"
    );
    assert_eq!(
        wi.node_id, 1,
        "and its node id must agree (0009's invariant)"
    );
}

/// The half that would actually hurt: 0015 must be a no-op wherever `node`
/// already has rows. Re-running it against a populated database must not rewind
/// the sequence onto ids that are already taken.
#[tokio::test]
async fn the_sequence_fix_does_not_touch_a_populated_database() {
    let (_pg, pool) = fresh_korg().await;

    let first = create_work_item(&pool, new::work_item("first"))
        .await
        .expect("create first");

    // Apply 0015 again by hand, as a re-run would.
    pool.execute(SEQUENCE_FIX).await.expect("re-apply 0015");

    let second = create_work_item(&pool, new::work_item("second"))
        .await
        .expect("create second");
    assert!(
        second.node_id > first.node_id,
        "re-running 0015 rewound the sequence: {} then {}",
        first.node_id,
        second.node_id
    );
}
