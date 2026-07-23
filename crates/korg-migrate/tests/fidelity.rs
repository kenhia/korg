//! S6 — fidelity gate (acceptance check for Milestone 1).
//!
//! Restores the frozen kwi/kcard snapshots, imports them into a fresh korg DB,
//! and proves the import is faithful to both sources:
//!
//!   F1 count parity      F2 wi_number preservation + sequence
//!   F3 field integrity   F4 relationship preservation
//!   F5 project merge      F6 areas (project-scoped)        F7 hierarchy
//!
//! Cards (which get new ids) are matched positionally: source cards ordered by
//! id correspond 1:1 to korg cards ordered by node id (insertion order).

mod common;

use std::collections::{HashMap, HashSet};

use korg_migrate::import::import;
use korg_migrate::source::{read_kcard, read_kwi};
use korg_test_support::count;
use rust_decimal::Decimal;
use sqlx::Row;
use time::OffsetDateTime;

#[derive(sqlx::FromRow)]
struct KorgWi {
    wi_number: i64,
    wi_type: String,
    wi_status: String,
    wi_tshirt: String,
    sprint: Option<String>,
    title: String,
    content: String,
    details: Option<String>,
    area_name: Option<String>,
    area_project: Option<String>,
    project_name: Option<String>,
    archived: bool,
    created: OffsetDateTime,
    updated: OffsetDateTime,
    parent_wi: Option<i64>,
}

#[derive(sqlx::FromRow)]
struct KorgCard {
    node_id: i64,
    status: String,
    title: String,
    description: String,
    rank: Decimal,
    project_name: Option<String>,
    category: Option<String>,
    tags: Vec<String>,
    archived: bool,
    created: OffsetDateTime,
    updated: OffsetDateTime,
}

#[derive(sqlx::FromRow)]
struct KorgComment {
    node_id: i64,
    body: String,
    created: OffsetDateTime,
    updated: OffsetDateTime,
}

#[derive(sqlx::FromRow)]
struct KorgProject {
    gh_repo: Option<String>,
    cn_path: Option<String>,
    description: Option<String>,
}

#[tokio::test]
async fn import_is_faithful_to_sources() {
    if common::skip_snapshot_suite("fidelity") {
        return;
    }
    let (pg, kwi_pool, kcard_pool) = common::staged_sources().await;
    let korg = common::migrate_korg(&pg).await;

    let kwi = read_kwi(&kwi_pool).await.expect("read kwi");
    let kcard = read_kcard(&kcard_pool).await.expect("read kcard");
    let report = import(&kwi, &kcard, &korg).await.expect("import");

    // Lookups over the source side.
    let kwi_project_name: HashMap<i32, String> = kwi
        .projects
        .iter()
        .map(|p| (p.id, p.project.clone()))
        .collect();
    // kwi area id -> (area name, project name)
    let kwi_area: HashMap<i32, (String, String)> = kwi
        .areas
        .iter()
        .map(|a| {
            (
                a.id,
                (a.name.clone(), kwi_project_name[&a.project_id].clone()),
            )
        })
        .collect();

    // ---- F1: count parity ----------------------------------------------
    assert_eq!(
        count(&korg, "workitem").await,
        kwi.workitems.len() as i64,
        "F1 workitems"
    );
    assert_eq!(
        count(&korg, "card").await,
        kcard.cards.len() as i64,
        "F1 cards"
    );
    assert_eq!(
        count(&korg, "comment").await,
        kcard.comments.len() as i64,
        "F1 comments"
    );
    assert_eq!(
        count(&korg, "relationship").await,
        kwi.related.len() as i64,
        "F1 edges"
    );
    assert_eq!(
        count(&korg, "area").await,
        kwi.areas.len() as i64,
        "F1 areas"
    );
    assert_eq!(
        count(&korg, "node").await,
        (kwi.workitems.len() + kcard.cards.len()) as i64,
        "F1 nodes"
    );

    // ---- F5: project merge ---------------------------------------------
    let mut expected_projects: HashSet<String> =
        kwi.projects.iter().map(|p| p.project.clone()).collect();
    for c in &kcard.cards {
        if let Some(p) = &c.project {
            expected_projects.insert(p.clone());
        }
    }
    assert_eq!(
        count(&korg, "project").await,
        expected_projects.len() as i64,
        "F5 merged project count"
    );
    // kwi project fields survive on the merged project.
    for p in &kwi.projects {
        let row = sqlx::query_as::<_, KorgProject>(
            "SELECT gh_repo, cn_path, description FROM project WHERE name = $1",
        )
        .bind(&p.project)
        .fetch_one(&korg)
        .await
        .unwrap_or_else(|_| panic!("F5 project `{}` missing", p.project));
        assert_eq!(row.gh_repo, p.gh_repo, "F5 gh_repo {}", p.project);
        assert_eq!(
            row.cn_path.as_deref(),
            Some(p.cn_path.as_str()),
            "F5 cn_path {}",
            p.project
        );
        assert_eq!(
            row.description, p.description,
            "F5 description {}",
            p.project
        );
    }

    // ---- F2/F3/F6/F7: work items ---------------------------------------
    let wi_rows = sqlx::query_as::<_, KorgWi>(
        "SELECT w.wi_number, w.wi_type, w.wi_status, w.wi_tshirt, w.sprint, \
                w.title, w.content, w.details, \
                a.name AS area_name, ap.name AS area_project, pj.name AS project_name, \
                n.archived, n.created, n.updated, pw.wi_number AS parent_wi \
         FROM workitem w \
         JOIN node n ON n.id = w.node_id \
         LEFT JOIN area a ON a.id = w.area_id \
         LEFT JOIN project ap ON ap.id = a.project_id \
         LEFT JOIN project pj ON pj.id = n.project_id \
         LEFT JOIN workitem pw ON pw.node_id = w.parent_node_id \
         ORDER BY w.wi_number",
    )
    .fetch_all(&korg)
    .await
    .expect("korg work items");
    let wi_by_number: HashMap<i64, &KorgWi> = wi_rows.iter().map(|w| (w.wi_number, w)).collect();

    // F2: wi_number set == kwi id set.
    let korg_numbers: HashSet<i64> = wi_rows.iter().map(|w| w.wi_number).collect();
    let source_numbers: HashSet<i64> = kwi.workitems.iter().map(|w| w.id as i64).collect();
    assert_eq!(korg_numbers, source_numbers, "F2 wi_number set");

    // F3 + F6 + F7 per work item.
    for w in &kwi.workitems {
        let k = wi_by_number
            .get(&(w.id as i64))
            .unwrap_or_else(|| panic!("F2 wi_number {} missing", w.id));
        assert_eq!(k.wi_type, w.wi_type, "F3 type wi {}", w.id);
        assert_eq!(k.wi_status, w.wi_status, "F3 status wi {}", w.id);
        assert_eq!(k.wi_tshirt, w.wi_tshirt, "F3 tshirt wi {}", w.id);
        assert_eq!(k.sprint, w.sprint, "F3 sprint wi {}", w.id);
        assert_eq!(k.title, w.title, "F3 title wi {}", w.id);
        assert_eq!(k.content, w.content, "F3 content wi {}", w.id);
        assert_eq!(k.details, w.details, "F3 details wi {}", w.id);
        assert_eq!(k.archived, w.archived, "F3 archived wi {}", w.id);
        assert_eq!(k.created, w.created, "F3 created wi {}", w.id);
        assert_eq!(k.updated, w.updated, "F3 updated wi {}", w.id);

        // Project (F3) — every work item's project preserved.
        assert_eq!(
            k.project_name.as_deref(),
            Some(kwi_project_name[&w.project_id].as_str()),
            "F3 project wi {}",
            w.id
        );

        // F6 areas — project-scoped.
        match w.area_id {
            Some(aid) => {
                let (area_name, area_project) = &kwi_area[&aid];
                assert_eq!(
                    k.area_name.as_deref(),
                    Some(area_name.as_str()),
                    "F6 area wi {}",
                    w.id
                );
                assert_eq!(
                    k.area_project.as_deref(),
                    Some(area_project.as_str()),
                    "F6 area project-scope wi {}",
                    w.id
                );
            }
            None => assert!(k.area_name.is_none(), "F6 area should be null wi {}", w.id),
        }

        // F7 hierarchy.
        assert_eq!(
            k.parent_wi,
            w.parent_id.map(|p| p as i64),
            "F7 parent wi {}",
            w.id
        );
    }

    // F2 (0009_identity): node ids ARE wi_numbers — every imported workitem is
    // aligned, and the single node sequence continues past the imported max.
    let misaligned: i64 = sqlx::query("SELECT COUNT(*) FROM workitem WHERE node_id <> wi_number")
        .fetch_one(&korg)
        .await
        .expect("alignment count")
        .get(0);
    assert_eq!(
        misaligned, 0,
        "F2 every imported workitem has node_id == wi_number"
    );
    let next: i64 = sqlx::query("SELECT nextval(pg_get_serial_sequence('node','id'))")
        .fetch_one(&korg)
        .await
        .expect("nextval")
        .get(0);
    assert!(
        next > report.max_wi_number,
        "F2 node sequence past imported max"
    );

    // ---- F3: cards (positional match) ----------------------------------
    let card_rows = sqlx::query_as::<_, KorgCard>(
        "SELECT n.id AS node_id, c.status::text AS status, c.title, c.description, c.rank, \
                p.name AS project_name, n.category, n.tags, n.archived, n.created, n.updated \
         FROM card c JOIN node n ON n.id = c.node_id \
         LEFT JOIN project p ON p.id = n.project_id \
         ORDER BY n.id",
    )
    .fetch_all(&korg)
    .await
    .expect("korg cards");
    assert_eq!(card_rows.len(), kcard.cards.len(), "F3 card count for zip");

    // source card id -> korg node id (for comment linkage).
    let mut card_id_to_node: HashMap<i64, i64> = HashMap::new();
    for (src, k) in kcard.cards.iter().zip(card_rows.iter()) {
        card_id_to_node.insert(src.id, k.node_id);
        assert_eq!(k.title, src.title, "F3 card title (src id {})", src.id);
        assert_eq!(k.status, src.status, "F3 card status (src id {})", src.id);
        assert_eq!(
            k.description, src.description,
            "F3 card description (src id {})",
            src.id
        );
        assert_eq!(k.rank, src.rank, "F3 card rank (src id {})", src.id);
        assert_eq!(
            k.project_name, src.project,
            "F3 card project (src id {})",
            src.id
        );
        assert_eq!(
            k.category, src.category,
            "F3 card category (src id {})",
            src.id
        );
        assert_eq!(k.tags, src.tags, "F3 card tags (src id {})", src.id);
        assert_eq!(
            k.archived, src.archived,
            "F3 card archived (src id {})",
            src.id
        );
        assert_eq!(
            k.created, src.created_at,
            "F3 card created (src id {})",
            src.id
        );
        assert_eq!(
            k.updated, src.updated_at,
            "F3 card updated (src id {})",
            src.id
        );
    }

    // ---- F3: comments (positional, with correct card linkage) ----------
    let comment_rows = sqlx::query_as::<_, KorgComment>(
        "SELECT node_id, body, created, updated FROM comment ORDER BY id",
    )
    .fetch_all(&korg)
    .await
    .expect("korg comments");
    assert_eq!(
        comment_rows.len(),
        kcard.comments.len(),
        "F3 comment count for zip"
    );
    for (src, k) in kcard.comments.iter().zip(comment_rows.iter()) {
        assert_eq!(k.body, src.body, "F3 comment body (src id {})", src.id);
        assert_eq!(
            k.created, src.created_at,
            "F3 comment created (src id {})",
            src.id
        );
        assert_eq!(
            k.updated, src.updated_at,
            "F3 comment updated (src id {})",
            src.id
        );
        assert_eq!(
            k.node_id, card_id_to_node[&src.card_id],
            "F3 comment card linkage (src id {})",
            src.id
        );
    }

    // ---- F4: relationships (over wi_number multiset) -------------------
    let rel_rows = sqlx::query(
        "SELECT lw.wi_number AS left_wi, rw.wi_number AS right_wi, r.relationship \
         FROM relationship r \
         JOIN workitem lw ON lw.node_id = r.left_id \
         JOIN workitem rw ON rw.node_id = r.right_id",
    )
    .fetch_all(&korg)
    .await
    .expect("korg relationships");
    let mut korg_edges: Vec<(i64, i64, String)> = rel_rows
        .iter()
        .map(|r| {
            (
                r.get::<i64, _>("left_wi"),
                r.get::<i64, _>("right_wi"),
                r.get::<String, _>("relationship"),
            )
        })
        .collect();
    let mut source_edges: Vec<(i64, i64, String)> = kwi
        .related
        .iter()
        .map(|r| (r.left_id as i64, r.right_id as i64, r.relationship.clone()))
        .collect();
    korg_edges.sort();
    source_edges.sort();
    assert_eq!(korg_edges, source_edges, "F4 relationship edges");
}
