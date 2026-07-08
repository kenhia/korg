//! 0009_identity remap test: seed a pre-0009 database with the production
//! shape — work items whose wi_numbers diverge from node ids, non-workitem
//! nodes squatting on wi_number-valued ids, relationships and comments across
//! kinds — then apply 0009 and assert one-number identity with every edge
//! intact. (schema.rs covers the clean full-migrator path; this test raw-applies
//! the SQL files so it can seed between 0008 and 0009.)

use sqlx::postgres::PgPoolOptions;
use sqlx::{Executor, Row};
use testcontainers_modules::postgres::Postgres;
use testcontainers_modules::testcontainers::runners::AsyncRunner;

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

#[tokio::test]
async fn identity_remap_preserves_edges() {
    let container = Postgres::default().start().await.expect("start postgres");
    let port = container.get_host_port_ipv4(5432).await.expect("port");
    let url = format!("postgres://postgres:postgres@127.0.0.1:{port}/postgres");
    let pool = PgPoolOptions::new()
        .max_connections(4)
        .connect(&url)
        .await
        .expect("connect");

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
