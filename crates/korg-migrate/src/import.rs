//! S5 — kwi + kcard -> korg importer.
//!
//! Maps the two source models onto korg's typed-node + generalized-edges
//! schema, preserving:
//!   * kwi work-item ids as the user-facing serial `wi_number` (+ sequence -> max+1),
//!   * kwi project/area taxonomy (projects merged with kcard by name),
//!   * kwi `related` edges and work-item parent hierarchy,
//!   * kcard cards (new ids), comments, tags, category, rank, status, archived.
//!
//! The whole import runs in one transaction so a failure leaves korg empty.

use std::collections::HashMap;

use anyhow::{Context, Result};
use sqlx::{PgPool, Row};

use crate::source::{KcardData, KwiData};

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct ImportReport {
    pub projects: i64,
    pub areas: i64,
    pub workitems: i64,
    pub cards: i64,
    pub comments: i64,
    pub relationships: i64,
    pub max_wi_number: i64,
}

pub async fn import(kwi: &KwiData, kcard: &KcardData, korg: &PgPool) -> Result<ImportReport> {
    let mut tx = korg.begin().await?;

    // --- 1. Projects: merge kwi + kcard by name --------------------------
    // name -> korg project id
    let mut project_by_name: HashMap<String, i64> = HashMap::new();

    for p in &kwi.projects {
        let id: i64 = sqlx::query(
            "INSERT INTO project (name, gh_repo, cn_path, description, created, updated) \
             VALUES ($1, $2, $3, $4, $5, $6) RETURNING id",
        )
        .bind(&p.project)
        .bind(&p.gh_repo)
        .bind(&p.cn_path)
        .bind(&p.description)
        .bind(p.created)
        .bind(p.updated)
        .fetch_one(&mut *tx)
        .await
        .context("insert kwi project")?
        .get("id");
        project_by_name.insert(p.project.clone(), id);
    }

    // kcard-origin projects that kwi never had.
    for card in &kcard.cards {
        if let Some(name) = card.project.as_ref() {
            if !project_by_name.contains_key(name) {
                let id: i64 =
                    sqlx::query("INSERT INTO project (name) VALUES ($1) RETURNING id")
                        .bind(name)
                        .fetch_one(&mut *tx)
                        .await
                        .context("insert kcard-only project")?
                        .get("id");
                project_by_name.insert(name.clone(), id);
            }
        }
    }

    // kwi project id -> korg project id (via name).
    let kwi_project_name: HashMap<i32, String> =
        kwi.projects.iter().map(|p| (p.id, p.project.clone())).collect();

    // --- 2. Areas (project-scoped) ---------------------------------------
    let mut area_map: HashMap<i32, i64> = HashMap::new();
    for a in &kwi.areas {
        let project_name = kwi_project_name
            .get(&a.project_id)
            .context("area references unknown kwi project")?;
        let korg_project_id = project_by_name[project_name];
        let id: i64 = sqlx::query(
            "INSERT INTO area (project_id, name, description) VALUES ($1, $2, $3) RETURNING id",
        )
        .bind(korg_project_id)
        .bind(&a.name)
        .bind(&a.description)
        .fetch_one(&mut *tx)
        .await
        .context("insert area")?
        .get("id");
        area_map.insert(a.id, id);
    }

    // --- 3. Work items (node + workitem) ---------------------------------
    let mut wi_node: HashMap<i32, i64> = HashMap::new();
    for w in &kwi.workitems {
        let project_name = kwi_project_name
            .get(&w.project_id)
            .context("workitem references unknown kwi project")?;
        let korg_project_id = project_by_name[project_name];

        let node_id: i64 = sqlx::query(
            "INSERT INTO node (kind, project_id, archived, created, updated) \
             VALUES ('workitem', $1, $2, $3, $4) RETURNING id",
        )
        .bind(korg_project_id)
        .bind(w.archived)
        .bind(w.created)
        .bind(w.updated)
        .fetch_one(&mut *tx)
        .await
        .context("insert workitem node")?
        .get("id");

        let area_id = w.area_id.and_then(|a| area_map.get(&a).copied());

        sqlx::query(
            "INSERT INTO workitem \
             (node_id, wi_number, area_id, wi_type, wi_status, wi_tshirt, sprint, \
              title, content, details) \
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)",
        )
        .bind(node_id)
        .bind(w.id as i64)
        .bind(area_id)
        .bind(&w.wi_type)
        .bind(&w.wi_status)
        .bind(&w.wi_tshirt)
        .bind(&w.sprint)
        .bind(&w.title)
        .bind(&w.content)
        .bind(&w.details)
        .execute(&mut *tx)
        .await
        .context("insert workitem")?;

        wi_node.insert(w.id, node_id);
    }

    // Second pass: parent hierarchy (now every node id is known).
    for w in &kwi.workitems {
        if let Some(parent) = w.parent_id {
            if let Some(parent_node) = wi_node.get(&parent) {
                sqlx::query("UPDATE workitem SET parent_node_id = $1 WHERE node_id = $2")
                    .bind(parent_node)
                    .bind(wi_node[&w.id])
                    .execute(&mut *tx)
                    .await
                    .context("set workitem parent")?;
            }
        }
    }

    // Keep wi_number serial going forward: next value = max + 1.
    let max_wi_number: i64 = kwi.workitems.iter().map(|w| w.id as i64).max().unwrap_or(0);
    if max_wi_number > 0 {
        sqlx::query("SELECT setval('workitem_wi_number_seq', $1, true)")
            .bind(max_wi_number)
            .execute(&mut *tx)
            .await
            .context("advance wi_number sequence")?;
    }

    // --- 4. Cards (node + card) ------------------------------------------
    let mut card_node: HashMap<i64, i64> = HashMap::new();
    for c in &kcard.cards {
        let korg_project_id = c
            .project
            .as_ref()
            .map(|name| project_by_name[name]);

        let node_id: i64 = sqlx::query(
            "INSERT INTO node (kind, project_id, category, tags, archived, created, updated) \
             VALUES ('card', $1, $2, $3, $4, $5, $6) RETURNING id",
        )
        .bind(korg_project_id)
        .bind(&c.category)
        .bind(&c.tags)
        .bind(c.archived)
        .bind(c.created_at)
        .bind(c.updated_at)
        .fetch_one(&mut *tx)
        .await
        .context("insert card node")?
        .get("id");

        sqlx::query(
            "INSERT INTO card (node_id, status, title, description, rank) \
             VALUES ($1, $2::card_status, $3, $4, $5)",
        )
        .bind(node_id)
        .bind(&c.status)
        .bind(&c.title)
        .bind(&c.description)
        .bind(c.rank)
        .execute(&mut *tx)
        .await
        .context("insert card")?;

        card_node.insert(c.id, node_id);
    }

    // --- 5. Comments ------------------------------------------------------
    for cm in &kcard.comments {
        let card_node_id = card_node
            .get(&cm.card_id)
            .context("comment references unknown card")?;
        sqlx::query(
            "INSERT INTO comment (card_node_id, body, created, updated) \
             VALUES ($1, $2, $3, $4)",
        )
        .bind(card_node_id)
        .bind(&cm.body)
        .bind(cm.created_at)
        .bind(cm.updated_at)
        .execute(&mut *tx)
        .await
        .context("insert comment")?;
    }

    // --- 6. Relationships (kwi `related`, generalized over node ids) -----
    for r in &kwi.related {
        let left = wi_node
            .get(&r.left_id)
            .context("relationship references unknown left work item")?;
        let right = wi_node
            .get(&r.right_id)
            .context("relationship references unknown right work item")?;
        sqlx::query(
            "INSERT INTO relationship (left_id, right_id, relationship) VALUES ($1, $2, $3)",
        )
        .bind(left)
        .bind(right)
        .bind(&r.relationship)
        .execute(&mut *tx)
        .await
        .context("insert relationship")?;
    }

    tx.commit().await?;

    Ok(ImportReport {
        projects: project_by_name.len() as i64,
        areas: kwi.areas.len() as i64,
        workitems: kwi.workitems.len() as i64,
        cards: kcard.cards.len() as i64,
        comments: kcard.comments.len() as i64,
        relationships: kwi.related.len() as i64,
        max_wi_number,
    })
}
