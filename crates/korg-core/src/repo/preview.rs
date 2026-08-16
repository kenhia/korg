//! Cross-kind node preview (WI #260).

use anyhow::Result;
use serde::Serialize;
use sqlx::{PgPool, Row};
use time::OffsetDateTime;
use ts_rs::TS;

use super::schedules::substituted;

// --- cross-kind node preview (WI #260) -------------------------------------

/// A label/value metadata row in a node preview (e.g. "Area" → "ui").
#[derive(Debug, Clone, Serialize, PartialEq, Eq, TS)]
#[ts(export, export_to = "korg.ts")]
pub struct NodeField {
    pub label: String,
    pub value: String,
}

/// A uniform, kind-agnostic preview of any node, used by the "find by ID"
/// search + preview panel: enough to identify and read an item without knowing
/// its kind up front. `wi_number` is `Some` only for work items (where it
/// equals the node id) — the UI navigates to those rather than previewing.
/// `body`/`details` are markdown; `badges` are short status chips; `fields`
/// are label/value metadata rows.
#[derive(Debug, Clone, Serialize, TS)]
#[ts(export, export_to = "korg.ts")]
pub struct NodePreview {
    pub node_id: i64,
    pub kind: String,
    pub wi_number: Option<i64>,
    pub title: String,
    pub project: Option<String>,
    pub tags: Vec<String>,
    pub archived: bool,
    pub badges: Vec<String>,
    pub fields: Vec<NodeField>,
    pub body: Option<String>,
    pub body_label: Option<String>,
    pub details: Option<String>,
    #[serde(with = "time::serde::rfc3339")]
    #[ts(type = "string")]
    pub created: OffsetDateTime,
    #[serde(with = "time::serde::rfc3339")]
    #[ts(type = "string")]
    pub updated: OffsetDateTime,
}

fn field(label: &str, value: impl Into<String>) -> NodeField {
    NodeField {
        label: label.into(),
        value: value.into(),
    }
}

/// Resolve any node id to a uniform preview, dispatching on its kind. Returns
/// `None` if no node has that id. Dates are read as `YYYY-MM-DD` text so the
/// payload needs no client-side date parsing.
pub async fn get_node_preview(pool: &PgPool, id: i64) -> Result<Option<NodePreview>> {
    let base = sqlx::query(
        "SELECT n.kind, pj.name AS project, n.tags, n.archived, n.created, n.updated \
         FROM node n LEFT JOIN project pj ON pj.id = n.project_id \
         WHERE n.id = $1",
    )
    .bind(id)
    .fetch_optional(pool)
    .await?;
    let Some(base) = base else { return Ok(None) };

    let kind: String = base.get("kind");
    let mut p = NodePreview {
        node_id: id,
        kind: kind.clone(),
        wi_number: None,
        title: format!("{kind} #{id}"),
        project: base.get("project"),
        tags: base.get("tags"),
        archived: base.get("archived"),
        badges: Vec::new(),
        fields: Vec::new(),
        body: None,
        body_label: None,
        details: None,
        created: base.get("created"),
        updated: base.get("updated"),
    };

    match kind.as_str() {
        "workitem" => {
            if let Some(r) = sqlx::query(
                "SELECT w.wi_number, w.wi_type, w.wi_status, w.wi_tshirt, w.sprint, \
                        a.name AS area, w.title, w.content, w.details \
                 FROM workitem w LEFT JOIN area a ON a.id = w.area_id \
                 WHERE w.node_id = $1",
            )
            .bind(id)
            .fetch_optional(pool)
            .await?
            {
                p.wi_number = Some(r.get("wi_number"));
                p.title = r.get("title");
                p.badges = vec![r.get("wi_type"), r.get("wi_status"), r.get("wi_tshirt")];
                if let Some(area) = r.get::<Option<String>, _>("area") {
                    p.fields.push(field("Area", area));
                }
                if let Some(sprint) = r.get::<Option<String>, _>("sprint") {
                    p.fields.push(field("Sprint", sprint));
                }
                p.body = Some(r.get("content"));
                p.body_label = Some("Content".into());
                p.details = r.get("details");
            }
        }
        "card" => {
            if let Some(r) = sqlx::query(
                "SELECT status::text AS status, title, description FROM card WHERE node_id = $1",
            )
            .bind(id)
            .fetch_optional(pool)
            .await?
            {
                p.title = r.get("title");
                p.badges = vec![r.get("status")];
                let desc: String = r.get("description");
                if !desc.trim().is_empty() {
                    p.body = Some(desc);
                    p.body_label = Some("Description".into());
                }
            }
        }
        "link" => {
            if let Some(r) = sqlx::query(
                "SELECT url, title, read, disposition::text AS disposition FROM link WHERE node_id = $1",
            )
            .bind(id)
            .fetch_optional(pool)
            .await?
            {
                let url: String = r.get("url");
                p.title = r.get::<Option<String>, _>("title").unwrap_or_else(|| url.clone());
                p.badges = vec![
                    r.get("disposition"),
                    if r.get::<bool, _>("read") { "read".into() } else { "unread".into() },
                ];
                p.fields.push(field("URL", url));
            }
        }
        "report" => {
            if let Some(r) = sqlx::query(
                "SELECT source, to_char(report_date, 'YYYY-MM-DD') AS report_date, status, \
                        summary, body, model, escalated \
                 FROM report WHERE node_id = $1",
            )
            .bind(id)
            .fetch_optional(pool)
            .await?
            {
                let source: String = r.get("source");
                let date: String = r.get("report_date");
                p.title = format!("{source} — {date}");
                p.badges = vec![r.get("status")];
                if r.get::<bool, _>("escalated") {
                    p.badges.push("escalated".into());
                }
                if let Some(model) = r.get::<Option<String>, _>("model") {
                    p.fields.push(field("Model", model));
                }
                p.fields.push(field("Summary", r.get::<String, _>("summary")));
                p.body = Some(r.get("body"));
                p.body_label = Some("Report".into());
            }
        }
        "sprint_proposal" => {
            if let Some(r) = sqlx::query(
                "SELECT sp.title, sp.summary, sp.notes, sp.status::text AS status, sp.pinned, \
                        (SELECT count(*) FROM relationship r JOIN node wn ON wn.id = r.right_id \
                          WHERE r.left_id = sp.node_id AND r.relationship = 'covers' \
                            AND wn.kind = 'workitem') AS covered_count, \
                        (SELECT coalesce(string_agg('#' || w.wi_number, ', ' \
                                                    ORDER BY w.wi_number), '') \
                           FROM relationship r JOIN workitem w ON w.node_id = r.right_id \
                          WHERE r.left_id = sp.node_id AND r.relationship = 'covers') AS covered \
                 FROM sprint_proposal sp WHERE sp.node_id = $1",
            )
            .bind(id)
            .fetch_optional(pool)
            .await?
            {
                p.title = r.get("title");
                p.badges = vec![r.get("status")];
                if r.get::<bool, _>("pinned") {
                    p.badges.push("pinned".into());
                }
                // The covered work items — the thing a proposal *is*, and the
                // half of the new shape this preview was still missing (#870).
                // Ids rather than titles: the panel is `max-w-md` and eight
                // wrapped titles push the notes off the screen, while the ids
                // are what you type into find-by-ID next.
                let covered_count: i64 = r.get("covered_count");
                if covered_count > 0 {
                    p.fields
                        .push(field("Covers", r.get::<String, _>("covered")));
                }
                // Since #860 the summary is a 500-char contract and the analysis
                // lives in `notes`, so the body is `notes` where there is one —
                // otherwise this preview would show the shortest form korg has
                // and nothing else. The summary stays visible as a field.
                match r.get::<Option<String>, _>("notes") {
                    Some(notes) => {
                        p.fields.push(field("Summary", r.get::<String, _>("summary")));
                        p.body = Some(notes);
                        p.body_label = Some("Notes".into());
                    }
                    None => {
                        p.body = Some(r.get("summary"));
                        p.body_label = Some("Summary".into());
                    }
                }
            }
        }
        "program" => {
            // WI #982. Sprint 044 added the `program` kind and its own page but
            // not an arm here, so find-by-ID answered `979` with the fallback:
            // chip PROGRAM, title literally "program #979".
            //
            // The original of this comment ended "there is no `_ => {}` kind
            // left to fix after this one", which was true when written and false
            // eleven days later: sprint 051 added `schedule` and no arm, and the
            // claim sat here reassuring readers for three sprints (#870's audit,
            // sprint 054). The claim is now `NODE_KINDS` plus the fence that
            // iterates it — a sentence in a comment cannot notice a new kind.
            //
            // `span` is the derived project set (D-6): a program has no project
            // of its own, so `p.project` is always null here and the span is the
            // honest answer to "which repos does this touch". Same statement
            // shape the program list uses.
            if let Some(r) = sqlx::query(
                "SELECT g.title, g.aim, g.notes, g.status, g.pinned, \
                        (SELECT count(*) FROM relationship r JOIN node sn ON sn.id = r.right_id \
                          WHERE r.left_id = g.node_id AND r.relationship = 'includes' \
                            AND sn.kind = 'sprint_proposal') AS slice_count, \
                        (SELECT coalesce(array_agg(DISTINCT pj.name), '{}') \
                           FROM relationship r JOIN node sn ON sn.id = r.right_id \
                           JOIN project pj ON pj.id = sn.project_id \
                          WHERE r.left_id = g.node_id AND r.relationship = 'includes') AS span \
                 FROM program g WHERE g.node_id = $1",
            )
            .bind(id)
            .fetch_optional(pool)
            .await?
            {
                p.title = r.get("title");
                p.badges = vec![r.get("status")];
                if r.get::<bool, _>("pinned") {
                    p.badges.push("pinned".into());
                }
                let span: Vec<String> = r.get("span");
                if !span.is_empty() {
                    p.fields.push(field("Span", span.join(", ")));
                }
                p.fields
                    .push(field("Slices", r.get::<i64, _>("slice_count").to_string()));
                // Same rule as a proposal: the routing contract (`aim`) stays a
                // field when there is a long form to show instead, and becomes
                // the body when there is not — never both.
                match r.get::<Option<String>, _>("notes") {
                    Some(notes) => {
                        p.fields.push(field("Aim", r.get::<String, _>("aim")));
                        p.body = Some(notes);
                        p.body_label = Some("Notes".into());
                    }
                    None => {
                        p.body = Some(r.get("aim"));
                        p.body_label = Some("Aim".into());
                    }
                }
            }
        }
        "handoff" => {
            // Sprint 026: the handoff "viewer" is this generic slide-over. The
            // owning read (get_work_item/get_proposal) surfaces the has_handoff
            // ref; clicking it opens this preview. A missing detail row leaves
            // the default `handoff #<id>` title rather than a blank node.
            if let Some(r) =
                sqlx::query("SELECT title, summary, body FROM handoff WHERE node_id = $1")
                    .bind(id)
                    .fetch_optional(pool)
                    .await?
            {
                p.title = r.get("title");
                p.fields.push(field("Summary", r.get::<String, _>("summary")));
                p.body = Some(r.get("body"));
                p.body_label = Some("Handoff".into());
            }
        }
        "schedule" => {
            // WI #870's audit, sprint 054 — the gap #982's fence was written to
            // catch and missed, because the fence hardcoded seven kinds and
            // sprint 051 added an eighth. Until now a `materializes` edge or a
            // find-by-ID on a schedule rendered "schedule #1042" with no fields
            // at all: the newest node type in korg, and the least visible.
            //
            // `preview_title` over `title` for the same reason the schedules
            // page shows it — the template is the stored value but the
            // substituted string is what Materialise would actually create, and
            // a preview exists to answer "what is this". The raw template stays
            // reachable as a field when the two differ, so the substitution is
            // never invisible.
            //
            // Due-ness is deliberately NOT recomputed here. `due` is a
            // read-time predicate over three clauses (0025) and the schedules
            // page is where it is rendered with the context that makes it mean
            // something; a bare "due" chip in a slide-over reached from an edge
            // would be a second, thinner implementation of the same derivation —
            // exactly the drift `SCHEDULE_DUE_SQL` exists to prevent. The dates
            // are here, and the page is one click away.
            if let Some(r) = sqlx::query(&format!(
                "SELECT s.title, {} AS preview_title, s.template, s.notes, \
                        s.cadence, s.anchor_mode, s.status, s.wi_type, s.wi_tshirt, \
                        to_char(s.anchor_at, 'YYYY-MM-DD') AS anchor_at, \
                        (SELECT count(*) FROM relationship r \
                          WHERE r.left_id = s.node_id \
                            AND r.relationship = 'materializes') AS run_count \
                 FROM schedule s WHERE s.node_id = $1",
                substituted("s.title")
            ))
            .bind(id)
            .fetch_optional(pool)
            .await?
            {
                // Substituted database-side, like every other schedule read, so
                // the rendered date comes off the same clock as the rows beside
                // it (`substituted`'s own reason).
                let template_title: String = r.get("title");
                let rendered: String = r.get("preview_title");
                p.title = rendered.clone();
                p.badges = vec![r.get("cadence"), r.get("status")];
                if rendered != template_title {
                    p.fields.push(field("Template", template_title));
                }
                let cadence: String = r.get("cadence");
                if cadence != "once" {
                    p.fields
                        .push(field("Anchor", format!("last {}", r.get::<String, _>("anchor_mode"))));
                }
                p.fields.push(field("Anchored", r.get::<String, _>("anchor_at")));
                p.fields.push(field(
                    "Creates",
                    format!(
                        "{} · {}",
                        r.get::<String, _>("wi_type"),
                        r.get::<String, _>("wi_tshirt")
                    ),
                ));
                p.fields
                    .push(field("Runs", r.get::<i64, _>("run_count").to_string()));
                // Same rule as a proposal and a program: the long form is the
                // body when there is one, and the work-item template it would
                // create is the next most useful thing when there is not.
                match r.get::<Option<String>, _>("notes") {
                    Some(notes) => {
                        p.body = Some(notes);
                        p.body_label = Some("Notes".into());
                    }
                    None => {
                        if let Some(template) = r.get::<Option<String>, _>("template") {
                            p.body = Some(template);
                            p.body_label = Some("Work item template".into());
                        }
                    }
                }
            }
        }
        "attachment" => {
            // Sprint 056. The generic preview is what a `has_attachment` edge
            // opens into from any owner, and — until slice 2 builds the real
            // lightbox — the only place an image's metadata is visible at all.
            //
            // `body` stays None: this struct carries markdown, and a screenshot
            // is not markdown. The variant URLs are fields, which is enough for
            // the panel to render a link and for a person to paste one into a
            // browser; NodePreview gaining an image affordance is slice 2's
            // call to make, not something to half-do here.
            if let Some(r) = sqlx::query(
                "SELECT a.filename, a.mime, a.byte_size, a.width, a.height, \
                        (SELECT count(*) FROM relationship r \
                          WHERE r.right_id = a.node_id \
                            AND r.relationship = 'has_attachment') AS owners \
                 FROM attachment a WHERE a.node_id = $1",
            )
            .bind(id)
            .fetch_optional(pool)
            .await?
            {
                let img = korg_img::ImgId::from_node_id(id)?;
                p.title = r.get("filename");
                p.badges = vec![
                    img.to_string(),
                    if r.get::<i64, _>("owners") > 0 {
                        "linked".into()
                    } else {
                        "pending".into()
                    },
                ];
                p.fields.push(field(
                    "Dimensions",
                    format!(
                        "{}×{}",
                        r.get::<i32, _>("width"),
                        r.get::<i32, _>("height")
                    ),
                ));
                p.fields.push(field("Type", r.get::<String, _>("mime")));
                p.fields.push(field(
                    "Size",
                    format_bytes(r.get::<i64, _>("byte_size")),
                ));
                p.fields.push(field("Original", img.url()));
                for variant in korg_img::VARIANTS {
                    p.fields
                        .push(field(variant.as_str(), img.variant_url(variant)));
                }
            }
        }
        _ => {}
    }

    Ok(Some(p))
}

/// Bytes as a person reads them. Only ever rendered into a preview field, where
/// `2.4 MB` is the answer to "is this a screenshot or a photo" and `2517291` is
/// not.
fn format_bytes(bytes: i64) -> String {
    const KB: f64 = 1024.0;
    let bytes = bytes as f64;
    if bytes < KB {
        return format!("{bytes:.0} B");
    }
    for (unit, scale) in [("KB", KB), ("MB", KB * KB), ("GB", KB * KB * KB)] {
        if bytes < scale * KB {
            return format!("{:.1} {unit}", bytes / scale);
        }
    }
    format!("{:.1} TB", bytes / (KB * KB * KB * KB))
}
