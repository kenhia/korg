//! Full-text search over the whole node graph (WI #1177, sprint 066).
//!
//! One lexical read across every kind that carries prose, plus one document per
//! comment — comments hold the payload that titles never do, and indexing them
//! is the measured difference between this and a title scan.
//!
//! The engine is Postgres: a generated, GIN-indexed `search_tsv` on each detail
//! table (migration 0029). That choice was measured, not assumed — see the
//! sprint record for the sweep against khound's frozen suite. Its consequence
//! for this module is that there is **no index lifecycle here**: no build, no
//! refresh, no staleness field on a hit. Postgres maintains the vectors inside
//! the writing transaction, so a search issued after a write sees that write.

use anyhow::Result;

use serde::Serialize;
use sqlx::PgPool;
use time::OffsetDateTime;
use ts_rs::TS;

use super::page::{ArchivedFilter, PageQuery};

/// Ranking, fixed deliberately rather than exposed as a knob.
///
/// `1` divides by `1 + log(document length)` — Postgres's stand-in for BM25's
/// length normalisation — and `32` rescales to 0..1 without reordering. On the
/// frozen acceptance suite this scored 12/12 top-1 where the obvious default
/// (`ts_rank_cd(tsv, q, 32)`, no length term and no IDF) scored 4/12: under the
/// any-term relaxation, long documents otherwise win on word count alone.
const RANK: &str = "ts_rank(d.search_tsv, q.q, 1|32)";

/// How much of a document a hit carries. Bounded on purpose — the response is a
/// routing contract, and `get_work_item`/`get_report` are where bodies live.
/// Unlike a leading-N-characters snippet, `ts_headline` centres the fragment on
/// the matched terms, so the snippet answers the question more often than not.
const HEADLINE: &str =
    "MaxWords=45, MinWords=20, ShortWord=3, MaxFragments=1, StartSel=\"\", StopSel=\"\"";

/// Every searchable document, as one relation. `terminal` is each kind's own
/// "hidden from the live list" state, so search's default scope agrees with
/// `list_work_items`, `list_proposals`, `list_programs` and the kanban rather
/// than inventing a fourth rule.
const DOCS: &str = r#"
node_state AS (
    SELECT n.id, n.archived, pj.name AS project, n.updated,
           CASE WHEN w.node_id IS NOT NULL THEN w.wi_status = 'closed'
                WHEN p.node_id IS NOT NULL THEN p.status::text IN ('done', 'declined')
                WHEN g.node_id IS NOT NULL THEN g.status = 'done'
                WHEN c.node_id IS NOT NULL THEN c.status::text IN ('Done', 'Cut')
                ELSE false END AS terminal
      FROM node n
      LEFT JOIN project pj        ON pj.id = n.project_id
      LEFT JOIN workitem w        ON w.node_id = n.id
      LEFT JOIN sprint_proposal p ON p.node_id = n.id
      LEFT JOIN program g         ON g.node_id = n.id
      LEFT JOIN card c            ON c.node_id = n.id
),
d AS (
    SELECT w.node_id, NULL::bigint AS comment_id, 'workitem' AS kind,
           'WI-' || w.wi_number AS locator, w.title,
           coalesce(w.content, '') || E'\n' || coalesce(w.details, '') AS body,
           w.wi_status AS status, w.search_tsv
      FROM workitem w
    UNION ALL
    SELECT p.node_id, NULL, 'sprint_proposal', 'korg:' || p.node_id, p.title,
           coalesce(p.summary, '') || E'\n' || coalesce(p.notes, ''),
           p.status::text, p.search_tsv FROM sprint_proposal p
    UNION ALL
    SELECT c.node_id, NULL, 'card', 'korg:' || c.node_id, c.title, c.description,
           c.status::text, c.search_tsv FROM card c
    UNION ALL
    SELECT h.node_id, NULL, 'handoff', 'korg:' || h.node_id, h.title,
           coalesce(h.summary, '') || E'\n' || coalesce(h.body, ''), NULL,
           h.search_tsv FROM handoff h
    UNION ALL
    SELECT g.node_id, NULL, 'program', 'korg:' || g.node_id, g.title,
           coalesce(g.aim, '') || E'\n' || coalesce(g.notes, ''), g.status,
           g.search_tsv FROM program g
    UNION ALL
    SELECT r.node_id, NULL, 'report', 'korg:' || r.node_id, r.source,
           coalesce(r.summary, '') || E'\n' || coalesce(r.body, ''), r.status,
           r.search_tsv FROM report r
    UNION ALL
    SELECT s.node_id, NULL, 'schedule', 'korg:' || s.node_id, s.title,
           coalesce(s.template, '') || E'\n' || coalesce(s.notes, ''), s.status,
           s.search_tsv FROM schedule s
    UNION ALL
    SELECT l.node_id, NULL, 'link', 'korg:' || l.node_id, l.title, l.url, NULL,
           l.search_tsv FROM link l
    UNION ALL
    SELECT m.node_id, m.id, 'comment', 'korg:' || m.node_id || '#comment-' || m.id,
           NULL, m.body, NULL, m.search_tsv FROM comment m
)"#;

/// One search hit. Lean by design: `snippet` is bounded, and the locator plus
/// `node_id`/`comment_id` are enough to route a follow-up read.
#[derive(Debug, Clone, sqlx::FromRow, Serialize, TS)]
#[ts(export, export_to = "korg.ts")]
pub struct SearchHit {
    pub node_id: i64,
    /// Set when the hit is a comment, so a caller can tell "this item matched"
    /// from "someone said this on it" without parsing the locator.
    pub comment_id: Option<i64>,
    pub kind: String,
    /// `WI-836`, `korg:1395`, `korg:1395#comment-777` — what to open next, and
    /// the same spelling Ken and agents already use for the thing.
    pub locator: String,
    /// Absent for comments, which have no title of their own.
    pub title: Option<String>,
    /// The node's own status, where its kind has one.
    pub status: Option<String>,
    pub project: Option<String>,
    /// A fragment centred on the matched terms, not the head of the document.
    pub snippet: String,
    pub score: f32,
    #[serde(with = "time::serde::rfc3339")]
    #[ts(type = "string")]
    pub updated: OffsetDateTime,
}

/// What the default scope hid, as a cascade so nothing is counted twice:
/// `archived` is what the archived filter excluded, and `terminal` is counted
/// only over the rows that passed it.
///
/// A field is 0 when the caller asked to see that class, so each name stays
/// true under every setting.
#[derive(Debug, Clone, Serialize, TS)]
#[ts(export, export_to = "korg.ts")]
pub struct SearchOmitted {
    /// Rows in their kind's own terminal state — `closed` work items,
    /// `done`/`declined` proposals, `done` programs, `Done`/`Cut` cards.
    pub terminal: i64,
    pub archived: i64,
}

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export, export_to = "korg.ts")]
pub struct SearchResults {
    pub items: Vec<SearchHit>,
    pub total: i64,
    pub limit: i64,
    pub offset: i64,
    pub omitted: SearchOmitted,
    /// True when the all-terms parse matched nothing and this answer is the
    /// any-term relaxation (WI 1001's contract).
    ///
    /// It is reported rather than hidden because the failure this feature was
    /// built out of was a silently-unrelaxed query path returning zero while
    /// claiming health (khound's Gate A M3). A caller that cannot tell strict
    /// from relaxed cannot tell a precise answer from a broad one.
    pub relaxed: bool,
    /// The tsquery that actually produced these results, as Postgres understood
    /// it — stop words dropped, stemming applied, phrases preserved. Present so
    /// a surprising result set can be diagnosed without a database session.
    ///
    /// When `relaxed` is true this is the any-term query, not the all-terms one
    /// that found nothing: reporting the parse that was *abandoned* would
    /// describe a search that did not happen.
    pub parsed: String,
}

#[derive(Debug, Clone, Default)]
pub struct SearchQuery {
    pub q: String,
    pub kind: Option<String>,
    pub project: Option<String>,
    pub scope: Option<String>,
    pub archived: ArchivedFilter,
    pub page: PageQuery,
}

/// Whether the default scope applies. Absent → live rows only; `"all"` → the
/// whole corpus.
///
/// The acceptance suite runs with `"all"` and must: every one of its keyed
/// nodes is closed or resolved, so a not-done default reaching the engine would
/// fail it outright. That is exactly why this is a *filter*, applied here, and
/// never folded into ranking.
fn live_only(scope: Option<&str>) -> bool {
    !matches!(scope, Some("all"))
}

pub async fn search(pool: &PgPool, q: SearchQuery) -> Result<SearchResults> {
    let (limit, offset) = q.page.resolve_public();
    let live = live_only(q.scope.as_deref());

    // Parse once, in Postgres, so the relaxation operates on what actually
    // matched rather than on a second Rust-side tokenisation that could drift
    // from it.
    let parsed: String = sqlx::query_scalar("SELECT websearch_to_tsquery('english', $1)::text")
        .bind(&q.q)
        .fetch_one(pool)
        .await?;

    let mut relaxed = false;
    let mut effective = parsed.clone();
    let mut counts = count(pool, &effective, &q, live).await?;

    // AND by default, any-term when AND comes back empty (WI 1001). Relaxing is
    // a text rewrite of the *parsed* query — `&` becomes `|`, so stemming, stop
    // words and quoted phrases survive intact.
    //
    // A query carrying an explicit negation is left alone: `foo -bar` relaxed
    // to `foo | !bar` would match every document without `bar`, which inverts
    // what the caller asked for. Someone who typed a negation meant a precise
    // query, so the honest response to "no matches" is no matches.
    if counts.0 == 0 && !parsed.is_empty() && !parsed.contains('!') {
        let or = parsed.replace('&', "|");
        if or != parsed {
            let n = count(pool, &or, &q, live).await?;
            if n.0 > 0 {
                relaxed = true;
                effective = or;
                counts = n;
            }
        }
    }

    let (total, omitted) = counts;
    let sql = format!(
        "WITH {DOCS}, q AS (SELECT $1::tsquery AS q)
         SELECT t.node_id, t.comment_id, t.kind, t.locator, t.title, t.status,
                t.project,
                ts_headline('english', t.body, q.q, '{HEADLINE}') AS snippet,
                t.score, t.updated
           FROM (
                SELECT d.node_id, d.comment_id, d.kind, d.locator, d.title, d.status,
                       d.body, ns.project, ns.updated, {RANK}::real AS score
                  FROM d JOIN node_state ns ON ns.id = d.node_id, q
                 WHERE d.search_tsv @@ q.q {FILTERS}
                 ORDER BY score DESC, d.node_id DESC, d.comment_id DESC NULLS FIRST
                 LIMIT $6 OFFSET $7
           ) t, q",
        DOCS = DOCS,
        RANK = RANK,
        HEADLINE = HEADLINE,
        FILTERS = FILTERS,
    );

    let items = bind_filters(sqlx::query_as::<_, SearchHit>(&sql), &effective, &q, live)
        .bind(limit)
        .bind(offset)
        .fetch_all(pool)
        .await?;

    Ok(SearchResults {
        items,
        total,
        limit,
        offset,
        omitted,
        relaxed,
        parsed: effective,
    })
}

/// The scope predicates, shared verbatim by the count and the page so the two
/// can never disagree about what was searched.
const FILTERS: &str = "
    AND ($2::boolean IS NULL OR ns.archived = $2)
    AND (NOT $3::boolean OR NOT ns.terminal)
    AND ($4::text IS NULL OR ns.project = $4)
    AND ($5::text IS NULL OR d.kind = $5)";

fn bind_filters<'a, O>(
    query: sqlx::query::QueryAs<'a, sqlx::Postgres, O, sqlx::postgres::PgArguments>,
    tsq: &'a str,
    q: &'a SearchQuery,
    live: bool,
) -> sqlx::query::QueryAs<'a, sqlx::Postgres, O, sqlx::postgres::PgArguments> {
    query
        .bind(tsq)
        .bind(q.archived)
        .bind(live)
        .bind(q.project.as_deref())
        .bind(q.kind.as_deref())
}

/// `total` plus the two `omitted` counts, in one statement over the matching
/// corpus. Counted separately from the page (never `count(*) OVER()`, which
/// returns zero exactly when the page overshoots — WI #883).
async fn count(
    pool: &PgPool,
    tsq: &str,
    q: &SearchQuery,
    live: bool,
) -> Result<(i64, SearchOmitted)> {
    // `omitted` is a cascade: `archived` counts what the archived filter
    // removed, `terminal` only what survived it — so an archived closed item
    // lands in `archived` and nowhere else.
    let sql = format!(
        "WITH {DOCS}, q AS (SELECT $1::tsquery AS q), m AS (
             SELECT ns.archived, ns.terminal
               FROM d JOIN node_state ns ON ns.id = d.node_id, q
              WHERE d.search_tsv @@ q.q
                AND ($4::text IS NULL OR ns.project = $4)
                AND ($5::text IS NULL OR d.kind = $5)
         )
         SELECT
           count(*) FILTER (
             WHERE ($2::boolean IS NULL OR archived = $2) AND (NOT $3::boolean OR NOT terminal)
           ) AS total,
           count(*) FILTER (
             WHERE ($2::boolean IS NULL OR archived = $2) AND $3::boolean AND terminal
           ) AS terminal,
           count(*) FILTER (WHERE NOT ($2::boolean IS NULL OR archived = $2)) AS archived
           FROM m"
    );
    let row: (i64, i64, i64) = bind_filters(sqlx::query_as(&sql), tsq, q, live)
        .fetch_one(pool)
        .await?;
    Ok((
        row.0,
        SearchOmitted {
            terminal: row.1,
            archived: row.2,
        },
    ))
}
