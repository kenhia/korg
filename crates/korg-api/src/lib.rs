//! korg-api — axum REST API over korg-core, and static host for the web bundle.

use std::path::PathBuf;
use std::sync::Arc;

use axum::extract::{DefaultBodyLimit, Path, Query, State};
use axum::http::{HeaderValue, Method};
use axum::routing::{delete, get, patch, post, put};
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::{json, Value};
use sqlx::PgPool;
use tower_http::cors::CorsLayer;
use tower_http::services::{ServeDir, ServeFile};
use tower_http::trace::TraceLayer;

use korg_core::config::KorgConfig;
use korg_core::ops;
use korg_core::repo::{
    self, CardPatch, HandoffPatch, LinkPatch, NewCard, NewHandoff, NewLink, NewProgram,
    NewProposal, NewSchedule, NewWorkItem, ProgramPatch, ProjectPatch, ProposalPatch,
    ReportSourcePatch, SchedulePatch, WorkItemPatch,
};
use korg_mcp::tools::KorgServer;
use rmcp::transport::streamable_http_server::session::local::LocalSessionManager;
use rmcp::transport::streamable_http_server::{StreamableHttpServerConfig, StreamableHttpService};

pub mod error;
pub mod img;
use error::ApiError;

#[derive(Clone)]
pub struct AppState {
    pub pool: Arc<PgPool>,
    pub config: Arc<KorgConfig>,
    /// The image blob store (sprint 056). In `AppState` rather than read from
    /// the environment at each call site so a test can point it at a temp
    /// directory — the same reason `config` is here.
    pub images: Arc<korg_img::Store>,
}

pub(crate) type ApiResult = Result<Json<Value>, ApiError>;

pub fn build_router(state: AppState) -> Router {
    let mcp = mcp_service(state.pool.clone(), state.config.timezone_name().to_owned());
    let api = Router::new()
        .route("/api/health", get(health))
        .route("/api/projects", get(list_projects).post(create_project))
        .route("/api/projects/recent", get(recent_project))
        .route(
            "/api/work-items",
            get(list_work_items).post(create_work_item),
        )
        .route("/api/work-items/survey", get(survey_work_items))
        .route("/api/work-items/flow", get(work_item_flow))
        .route(
            "/api/work-items/:wi_number",
            get(get_work_item).patch(update_work_item),
        )
        .route(
            "/api/areas",
            get(list_areas)
                .post(create_area)
                .patch(update_area)
                .delete(delete_area),
        )
        .route("/api/cards", get(list_cards).post(create_card))
        .route("/api/cards/:node_id", patch(update_card))
        .route("/api/nodes/:id", get(get_node))
        .route(
            "/api/nodes/:node_id/comments",
            get(list_comments).post(add_comment),
        )
        .route(
            "/api/comments/:id",
            delete(delete_comment).patch(update_comment),
        )
        .route("/api/links", get(list_links).post(create_link))
        .route(
            "/api/links/:node_id",
            patch(update_link).delete(delete_link),
        )
        .route("/api/relationships", post(create_relationship))
        .route("/api/relationships/:id", delete(delete_relationship))
        .route("/api/nodes/:id/neighbors", get(neighbors))
        .route("/api/projects/:name/plan", get(project_plan))
        .route("/api/projects/:name", patch(update_project))
        .route("/api/proposals", get(list_proposals).post(create_proposal))
        .route("/api/proposals/rollup", get(planning_rollup))
        .route("/api/programs", get(list_programs).post(create_program))
        .route(
            "/api/programs/:node_id",
            get(get_program).patch(update_program),
        )
        .route("/api/schedules", get(list_schedules).post(create_schedule))
        .route(
            "/api/schedules/:node_id",
            get(get_schedule).patch(update_schedule),
        )
        .route(
            "/api/schedules/:node_id/materialize",
            post(materialize_schedule),
        )
        .route("/api/report-sources", get(list_report_sources))
        .route("/api/report-sources/:source", patch(set_report_source))
        .route("/api/awaiting", get(list_awaiting))
        .route("/api/search", get(search))
        .route("/api/board", get(board))
        .route("/api/nodes/:id/awaiting", put(set_awaiting))
        .route("/api/reports", get(list_reports))
        .route("/api/reports/:node_id", get(get_report))
        .route("/api/handoffs", post(create_handoff))
        .route(
            "/api/handoffs/:node_id",
            get(get_handoff).patch(update_handoff),
        )
        .route(
            "/api/proposals/:node_id",
            get(get_proposal).patch(update_proposal),
        )
        // --- images (#582/#1119) ---
        //
        // Registered ahead of the `:id` routes so the intent is visible;
        // matchit prefers a static segment either way, which is what keeps
        // `/api/img/stats` from resolving as an attachment called "stats".
        //
        // The upload route carries its own body limit: axum's default is 2 MB,
        // and the whole point of a screenshot endpoint is that screenshots are
        // bigger than that. `korg_img::prepare` enforces the same cap again on
        // the buffered bytes, so a router change cannot silently lift it.
        .route("/api/img/stats", get(img::stats))
        .route("/api/img/sweep", post(img::sweep_now))
        .route(
            "/api/img",
            post(img::upload).layer(DefaultBodyLimit::max(korg_img::MAX_UPLOAD_BYTES)),
        )
        .route(
            "/api/img/:id",
            get(img::serve_original).delete(img::delete_image),
        )
        .route("/api/img/:id/link", post(img::link_image))
        .route("/api/img/:id/:variant", get(img::serve_variant))
        .with_state(state);

    let api = api.route_service("/mcp", mcp);

    let router = match web_dir() {
        Some(dir) => spa_fallback(api, &dir),
        None => api,
    };
    router.layer(TraceLayer::new_for_http()).layer(cors_layer())
}

/// Serve the SPA bundle from `dir`: real files (assets, favicon, index) come
/// straight off disk; anything else falls back to `index.html` so the client
/// router can take over. WI #284 — the fallback MUST use `ServeDir::fallback`,
/// not `not_found_service`: the latter serves the shell body but stamps the
/// upstream 404 onto it, so deep links / bookmarks (e.g. /planning) load the
/// page with a 404 status. `fallback` preserves the shell's 200.
fn spa_fallback(api: Router, dir: &std::path::Path) -> Router {
    let index = dir.join("index.html");
    let serve = ServeDir::new(dir).fallback(ServeFile::new(index));
    api.fallback_service(serve)
}

/// Build the MCP server as a Streamable-HTTP Tower service mounted at `/mcp`.
///
/// Configured for stateless JSON responses (no SSE framing, no session header):
/// each POST is an independent JSON-RPC request/response, which is the simplest
/// transport for a single-user tool and trivially testable with `curl`. Host
/// validation is disabled because korg is reached over several hostnames
/// (e.g. `kai`, `kubsdb`) on a trusted network — same posture as the REST API.
fn mcp_service(
    pool: Arc<PgPool>,
    timezone: String,
) -> StreamableHttpService<KorgServer, LocalSessionManager> {
    // `with_stateful_mode(false)` in rmcp 1.x; renamed in 3.x because SEP-2567
    // removes sessions from `2026-07-28` outright, so the flag only ever
    // governed the *legacy* lifecycle. Same value, honester name.
    let transport_config = StreamableHttpServerConfig::default()
        .with_legacy_session_mode(false)
        .with_json_response(true)
        .disable_allowed_hosts();
    StreamableHttpService::new(
        move || Ok(KorgServer::new((*pool).clone(), timezone.clone())),
        Arc::new(LocalSessionManager::default()),
        transport_config,
    )
}

fn web_dir() -> Option<PathBuf> {
    let candidate = std::env::var("KORG_WEB_DIR").unwrap_or_else(|_| "/app/web/build".to_string());
    let path = PathBuf::from(candidate);
    path.join("index.html").is_file().then_some(path)
}

fn cors_layer() -> CorsLayer {
    let origins_env = std::env::var("KORG_CORS_ORIGINS").unwrap_or_default();
    let origins: Vec<HeaderValue> = origins_env
        .split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .filter_map(|s| HeaderValue::from_str(s).ok())
        .collect();
    let layer = CorsLayer::new()
        .allow_methods([
            Method::GET,
            Method::POST,
            Method::PATCH,
            Method::PUT,
            Method::DELETE,
            Method::OPTIONS,
        ])
        .allow_headers([axum::http::header::CONTENT_TYPE]);
    if origins.is_empty() {
        layer
    } else {
        layer.allow_origin(origins)
    }
}

/// `archived` is tri-state across every collection read (D-3): absent means
/// `false` — the deliberate default change — `true` means archived only, and
/// `all` means both. Anything else is a 400 rather than a silent reinterpretation.
fn parse_archived(raw: Option<&str>) -> Result<Option<bool>, ApiError> {
    match raw {
        None | Some("false") => Ok(Some(false)),
        Some("true") => Ok(Some(true)),
        Some("all") => Ok(None),
        Some(other) => Err(ApiError::invalid(format!(
            "invalid archived '{other}' — expected one of: true, false, all"
        ))),
    }
}

/// 404 with a `not_found` code (D-6).
fn not_found(msg: String) -> ApiError {
    ApiError(korg_core::error::RepoError::NotFound(msg).into())
}

async fn health() -> Json<Value> {
    Json(json!({ "status": "ok" }))
}

// --- projects -------------------------------------------------------------

async fn list_projects(State(s): State<AppState>) -> ApiResult {
    Ok(Json(json!(repo::list_projects(&s.pool).await?)))
}

async fn create_project(State(s): State<AppState>, Json(b): Json<ops::CreateProject>) -> ApiResult {
    let id = repo::create_project(&s.pool, &b.name).await?;
    Ok(Json(json!({ "id": id, "name": b.name })))
}

async fn recent_project(State(s): State<AppState>) -> ApiResult {
    Ok(Json(
        json!({ "project": repo::recent_project(&s.pool).await? }),
    ))
}

// --- work items -----------------------------------------------------------

#[derive(Deserialize)]
struct WorkItemsQuery {
    project: Option<String>,
    archived: Option<String>,
    limit: Option<i64>,
    offset: Option<i64>,
}

async fn list_work_items(State(s): State<AppState>, Query(q): Query<WorkItemsQuery>) -> ApiResult {
    let page = repo::list_work_items(
        &s.pool,
        repo::WorkItemQuery {
            project: q.project,
            archived: parse_archived(q.archived.as_deref())?,
            page: repo::PageQuery {
                limit: q.limit,
                offset: q.offset,
            },
        },
    )
    .await?;
    Ok(Json(json!(page)))
}

#[derive(Deserialize)]
struct SurveyQuery {
    project: Option<String>,
    wi_status: Option<String>,
    /// Tri-state like every other collection read (WI #851). This was the one
    /// query struct taking a raw `Option<bool>`, so `?archived=all` was an error
    /// here and omitting it meant "both" rather than "live only".
    archived: Option<String>,
    limit: Option<i64>,
    offset: Option<i64>,
}

/// The lean projection, shared with the MCP `list_work_items` since #861 — so
/// omitting `wi_status` here now means "everything not terminal" rather than
/// "every status". Safe for the one caller: the Review page asks per status.
async fn survey_work_items(State(s): State<AppState>, Query(q): Query<SurveyQuery>) -> ApiResult {
    let limit = q.limit.unwrap_or(50).clamp(1, 500);
    let offset = q.offset.unwrap_or(0).max(0);
    let survey = repo::list_work_items_lean(
        &s.pool,
        q.project.as_deref(),
        q.wi_status.as_deref(),
        parse_archived(q.archived.as_deref())?,
        limit,
        offset,
    )
    .await?;
    Ok(Json(json!(survey)))
}

#[derive(Deserialize)]
struct FlowQuery {
    days: Option<i64>,
}

/// Full-text search (#1177). REST spells `archived` as `true|false|all` and
/// otherwise takes the same knobs as the MCP `search` tool, funnelling into the
/// same `repo::SearchQuery` — the web box and an agent get identical answers.
#[derive(Debug, serde::Deserialize)]
struct SearchParams {
    q: String,
    kind: Option<String>,
    project: Option<String>,
    scope: Option<String>,
    archived: Option<String>,
    limit: Option<i64>,
    offset: Option<i64>,
}

async fn search(State(s): State<AppState>, Query(p): Query<SearchParams>) -> ApiResult {
    let results = repo::search(
        &s.pool,
        repo::SearchQuery {
            q: p.q,
            kind: p.kind,
            project: p.project,
            scope: p.scope,
            archived: parse_archived(p.archived.as_deref())?,
            page: repo::PageQuery {
                limit: p.limit,
                offset: p.offset,
            },
        },
    )
    .await?;
    Ok(Json(json!(results)))
}

/// The backlog-trend read (#1318): one row per day in the board's timezone,
/// oldest first, ending today. A window reaching past the transition horizon
/// is clamped to it and the response names the horizon — days the log cannot
/// answer are absent, never zero-filled.
async fn work_item_flow(State(s): State<AppState>, Query(q): Query<FlowQuery>) -> ApiResult {
    let flow = repo::work_item_flow(&s.pool, q.days, s.config.timezone_name()).await?;
    Ok(Json(json!(flow)))
}

/// Missing work item is a 404, not `200 null` (D-6) — a typo'd number must
/// not read as "exists, but empty".
/// Returns the same shape as the MCP `get_work_item` tool (WI #535): the row
/// plus capped inline comments. They were the same operation under one name
/// with two shapes; the UI's separate comments fetch is now redundant on load.
async fn get_work_item(State(s): State<AppState>, Path(wi): Path<i64>) -> ApiResult {
    match repo::get_work_item_detail(&s.pool, wi).await? {
        Some(detail) => Ok(Json(json!(detail))),
        None => Err(not_found(format!("no work item #{wi}"))),
    }
}

async fn create_work_item(State(s): State<AppState>, Json(b): Json<NewWorkItem>) -> ApiResult {
    Ok(Json(json!(repo::create_work_item(&s.pool, b).await?)))
}

async fn update_work_item(
    State(s): State<AppState>,
    Path(wi): Path<i64>,
    Json(patch): Json<WorkItemPatch>,
) -> ApiResult {
    Ok(Json(json!(
        repo::update_work_item(&s.pool, wi, patch).await?
    )))
}

async fn list_areas(State(s): State<AppState>, Query(q): Query<ops::ProjectRef>) -> ApiResult {
    Ok(Json(json!(repo::list_areas(&s.pool, &q.project).await?)))
}

async fn create_area(State(s): State<AppState>, Json(b): Json<ops::CreateArea>) -> ApiResult {
    let id = repo::create_area(&s.pool, &b.project, &b.name, b.description.as_deref()).await?;
    Ok(Json(json!({ "id": id, "name": b.name })))
}

async fn update_area(State(s): State<AppState>, Json(b): Json<ops::UpdateArea>) -> ApiResult {
    Ok(Json(json!(
        repo::update_area(
            &s.pool,
            &b.project,
            &b.name,
            b.new_name.as_deref(),
            b.description.as_ref().map(|d| d.as_deref()),
        )
        .await?
    )))
}

async fn delete_area(State(s): State<AppState>, Json(b): Json<ops::AreaRef>) -> ApiResult {
    let deleted = repo::delete_area(&s.pool, &b.project, &b.name).await?;
    Ok(Json(json!({ "deleted": deleted })))
}

// --- cards ----------------------------------------------------------------

#[derive(Deserialize)]
struct CardsQuery {
    status: Option<String>,
    project: Option<String>,
    archived: Option<String>,
    limit: Option<i64>,
    offset: Option<i64>,
}

async fn list_cards(State(s): State<AppState>, Query(q): Query<CardsQuery>) -> ApiResult {
    let page = repo::list_cards(
        &s.pool,
        repo::CardQuery {
            status: q.status,
            project: q.project,
            archived: parse_archived(q.archived.as_deref())?,
            page: repo::PageQuery {
                limit: q.limit,
                offset: q.offset,
            },
        },
    )
    .await?;
    Ok(Json(json!(page)))
}

async fn create_card(State(s): State<AppState>, Json(b): Json<NewCard>) -> ApiResult {
    Ok(Json(json!(repo::create_card(&s.pool, b).await?)))
}

async fn update_card(
    State(s): State<AppState>,
    Path(node_id): Path<i64>,
    Json(patch): Json<CardPatch>,
) -> ApiResult {
    Ok(Json(json!(
        repo::update_card(&s.pool, node_id, patch).await?
    )))
}

async fn list_comments(State(s): State<AppState>, Path(node_id): Path<i64>) -> ApiResult {
    Ok(Json(json!(repo::list_comments(&s.pool, node_id).await?)))
}

async fn add_comment(
    State(s): State<AppState>,
    Path(node_id): Path<i64>,
    Json(b): Json<ops::CommentBody>,
) -> ApiResult {
    Ok(Json(json!(
        repo::add_comment(&s.pool, node_id, &b.body).await?
    )))
}

async fn update_comment(
    State(s): State<AppState>,
    Path(id): Path<i64>,
    Json(b): Json<ops::CommentBody>,
) -> ApiResult {
    Ok(Json(json!(
        repo::update_comment(&s.pool, id, &b.body).await?
    )))
}

async fn update_project(
    State(s): State<AppState>,
    Path(name): Path<String>,
    Json(patch): Json<ProjectPatch>,
) -> ApiResult {
    Ok(Json(json!(
        repo::update_project_by_name(&s.pool, &name, &patch).await?
    )))
}

async fn delete_comment(State(s): State<AppState>, Path(id): Path<i64>) -> ApiResult {
    let deleted = repo::delete_comment(&s.pool, id).await?;
    Ok(Json(json!({ "deleted": deleted })))
}

// --- links (reading list) -------------------------------------------------

#[derive(Deserialize)]
struct LinksQuery {
    disposition: Option<String>,
    read: Option<bool>,
    archived: Option<String>,
    limit: Option<i64>,
    offset: Option<i64>,
}

async fn list_links(State(s): State<AppState>, Query(q): Query<LinksQuery>) -> ApiResult {
    let page = repo::list_links(
        &s.pool,
        repo::LinkQuery {
            disposition: q.disposition,
            read: q.read,
            archived: parse_archived(q.archived.as_deref())?,
            page: repo::PageQuery {
                limit: q.limit,
                offset: q.offset,
            },
        },
    )
    .await?;
    Ok(Json(json!(page)))
}

async fn create_link(State(s): State<AppState>, Json(b): Json<NewLink>) -> ApiResult {
    Ok(Json(json!(repo::create_link(&s.pool, b).await?)))
}

async fn update_link(
    State(s): State<AppState>,
    Path(node_id): Path<i64>,
    Json(patch): Json<LinkPatch>,
) -> ApiResult {
    Ok(Json(json!(
        repo::update_link(&s.pool, node_id, patch).await?
    )))
}

async fn delete_link(State(s): State<AppState>, Path(node_id): Path<i64>) -> ApiResult {
    let deleted = repo::delete_link(&s.pool, node_id).await?;
    Ok(Json(json!({ "deleted": deleted })))
}

// --- relationships --------------------------------------------------------

async fn create_relationship(State(s): State<AppState>, Json(b): Json<ops::Relate>) -> ApiResult {
    let id = repo::relate(
        &s.pool,
        b.left,
        b.right,
        &b.label,
        b.origin.as_deref(),
        b.rank,
    )
    .await?;
    Ok(Json(json!({ "id": id })))
}

async fn delete_relationship(State(s): State<AppState>, Path(id): Path<i64>) -> ApiResult {
    let deleted = repo::unrelate(&s.pool, id).await?;
    Ok(Json(json!({ "deleted": deleted })))
}

/// A node's edges, optionally filtered by label and neighbor kind (WI #533).
/// Returns `{items, total, limit, truncated}` — the bound is explicit so a
/// caller can tell a complete answer from a clipped one.
async fn neighbors(
    State(s): State<AppState>,
    Path(id): Path<i64>,
    Query(q): Query<ops::Neighbors>,
) -> ApiResult {
    Ok(Json(json!(repo::neighbors(&s.pool, id, q.into()).await?)))
}

/// Kind-agnostic preview of any node by its id (WI #260). 404 when no node has
/// that id (D-6) — the find-by-ID box branches on the status.
async fn get_node(State(s): State<AppState>, Path(id): Path<i64>) -> ApiResult {
    match repo::get_node_preview(&s.pool, id).await? {
        Some(preview) => Ok(Json(json!(preview))),
        None => Err(not_found(format!("no node with id {id}"))),
    }
}

/// Plan view payload: a project's work items plus its `depends_on` edges
/// ([left, right] = left depends on right). Frontier/blocked computation
/// happens client-side — the full item set is already in the payload.
///
/// **Full means full (#1391).** This used to take one `LIST_LIMIT_MAX` page and
/// throw the envelope away, so a project past 500 items silently lost its
/// newest ones — the read is `ORDER BY wi_number`, so the clip lands exactly
/// where the live work is — and the payload said nothing a consumer could
/// notice it by. Both `/plan` and the `plan-status` skill derive a frontier
/// from this: a clipped item set is not a smaller answer, it is a wrong one.
/// The pages are walked to `total`, and `total` rides along so the payload can
/// still be checked against itself. korg, the largest project, holds 178 items,
/// so this is one page today and stays correct at 501.
///
/// **Archived rows are included, on purpose.** It was an undeclared
/// `archived: None` before; it is now a decision. An archived item is still a
/// dependency other items were satisfied by, and the graph needs it — dropping
/// it would turn a met dependency into a silently missing node. What to *show*
/// is the consumer's call, and `/plan` already filters archived out of its open
/// lanes.
async fn project_plan(State(s): State<AppState>, Path(name): Path<String>) -> ApiResult {
    let mut items = Vec::new();
    let total = loop {
        let page = repo::list_work_items(
            &s.pool,
            repo::WorkItemQuery {
                project: Some(name.clone()),
                archived: None,
                page: repo::PageQuery {
                    limit: Some(repo::LIST_LIMIT_MAX),
                    offset: Some(items.len() as i64),
                },
            },
        )
        .await?;
        let drained = page.items.is_empty();
        items.extend(page.items);
        // `drained` guards the loop against a corpus shrinking mid-walk, where
        // `total` alone would never be reached.
        if drained || items.len() as i64 >= page.total {
            break page.total;
        }
    };
    let edges = repo::project_edges(&s.pool, &name, "depends_on").await?;
    Ok(Json(
        json!({ "items": items, "edges": edges, "total": total }),
    ))
}

// --- daily reports ----------------------------------------------------------

#[derive(Deserialize)]
struct ReportsQuery {
    source: Option<String>,
    limit: Option<i64>,
}

async fn list_reports(State(s): State<AppState>, Query(q): Query<ReportsQuery>) -> ApiResult {
    Ok(Json(json!(
        repo::list_reports(&s.pool, q.source.as_deref(), q.limit.unwrap_or(30)).await?
    )))
}

async fn get_report(State(s): State<AppState>, Path(node_id): Path<i64>) -> ApiResult {
    match repo::get_report(&s.pool, node_id).await? {
        Some(r) => Ok(Json(json!(r))),
        None => Err(not_found(format!("no report with node_id {node_id}"))),
    }
}

// --- sprint proposals (agent planning) -------------------------------------

async fn list_proposals(
    State(s): State<AppState>,
    Query(q): Query<ops::ListProposals>,
) -> ApiResult {
    Ok(Json(json!(repo::list_proposals(&s.pool, q.into()).await?)))
}

/// Per-project planning weather for the Planning rail (WI #823) — one row per
/// project, `<proposals> | <wi_in_proposal> / <wi_total>`.
///
/// REST-only on purpose. This is a rail-rendering read, and the MCP catalogue
/// is a surface every agent pays to read; the same numbers are already
/// reachable there per row, via the `proposal_node_id` this sprint put on
/// `list_work_items`. Registered before `/api/proposals/:node_id` — static
/// segments win in matchit, but the ordering makes that intent visible.
async fn planning_rollup(State(s): State<AppState>) -> ApiResult {
    Ok(Json(json!(repo::planning_rollup(&s.pool).await?)))
}

/// The authoritative "what is this sprint" read (WI #536).
async fn get_proposal(State(s): State<AppState>, Path(node_id): Path<i64>) -> ApiResult {
    match repo::get_proposal_detail(&s.pool, node_id).await? {
        Some(detail) => Ok(Json(json!(detail))),
        None => Err(not_found(format!("no proposal with node_id {node_id}"))),
    }
}

async fn create_proposal(State(s): State<AppState>, Json(b): Json<NewProposal>) -> ApiResult {
    Ok(Json(json!(repo::create_proposal(&s.pool, b).await?)))
}

async fn update_proposal(
    State(s): State<AppState>,
    Path(node_id): Path<i64>,
    Json(patch): Json<ProposalPatch>,
) -> ApiResult {
    Ok(Json(json!(
        repo::update_proposal(&s.pool, node_id, patch).await?
    )))
}

// --- programs: the cross-project layer (#968) -------------------------------

async fn list_programs(State(s): State<AppState>, Query(q): Query<ops::ListPrograms>) -> ApiResult {
    Ok(Json(json!(
        repo::list_programs(&s.pool, q.status.as_deref(), q.archived).await?
    )))
}

/// The rollup read (D-5): a program, its ordered slices, and each slice's
/// work-item counts — so the UI renders a program without walking the graph.
async fn get_program(State(s): State<AppState>, Path(node_id): Path<i64>) -> ApiResult {
    match repo::get_program_detail(&s.pool, node_id).await? {
        Some(detail) => Ok(Json(json!(detail))),
        None => Err(not_found(format!("no program with node_id {node_id}"))),
    }
}

async fn create_program(State(s): State<AppState>, Json(b): Json<NewProgram>) -> ApiResult {
    Ok(Json(json!(repo::create_program(&s.pool, b).await?)))
}

async fn update_program(
    State(s): State<AppState>,
    Path(node_id): Path<i64>,
    Json(patch): Json<ProgramPatch>,
) -> ApiResult {
    Ok(Json(json!(
        repo::update_program(&s.pool, node_id, patch).await?
    )))
}

// --- schedules: work a date makes appear (#581) -----------------------------

async fn list_schedules(
    State(s): State<AppState>,
    Query(q): Query<ops::ListSchedules>,
) -> ApiResult {
    Ok(Json(json!(
        repo::list_schedules(
            &s.pool,
            q.status.as_deref(),
            q.project.as_deref(),
            q.due_only,
            q.archived,
        )
        .await?
    )))
}

/// The focused read: the schedule, its due-ness, and every work item it has
/// materialised — so a UI answers "when was this drill last actually run?"
/// without walking the `materializes` edges itself.
async fn get_schedule(State(s): State<AppState>, Path(node_id): Path<i64>) -> ApiResult {
    match repo::get_schedule_detail(&s.pool, node_id).await? {
        Some(detail) => Ok(Json(json!(detail))),
        None => Err(not_found(format!("no schedule with node_id {node_id}"))),
    }
}

async fn create_schedule(State(s): State<AppState>, Json(b): Json<NewSchedule>) -> ApiResult {
    Ok(Json(json!(repo::create_schedule(&s.pool, b).await?)))
}

async fn update_schedule(
    State(s): State<AppState>,
    Path(node_id): Path<i64>,
    Json(patch): Json<SchedulePatch>,
) -> ApiResult {
    Ok(Json(json!(
        repo::update_schedule(&s.pool, node_id, patch).await?
    )))
}

/// POST rather than PATCH: this **creates** a work item. The `force` flag is a
/// query parameter so the body can stay empty for the ordinary case.
async fn materialize_schedule(
    State(s): State<AppState>,
    Path(node_id): Path<i64>,
    Query(q): Query<MaterializeQuery>,
) -> ApiResult {
    Ok(Json(json!(
        repo::materialize_schedule(&s.pool, node_id, q.force).await?
    )))
}

#[derive(Debug, Default, Deserialize)]
struct MaterializeQuery {
    #[serde(default)]
    force: bool,
}

// --- report-source staleness (#950) -----------------------------------------

/// Every known source and whether korg can currently believe it. See
/// `repo::list_report_sources` — most-alarming first, and nothing but a `fresh`
/// source ever carries a real status.
async fn list_report_sources(State(s): State<AppState>) -> ApiResult {
    Ok(Json(json!(repo::list_report_sources(&s.pool).await?)))
}

async fn set_report_source(
    State(s): State<AppState>,
    Path(source): Path<String>,
    Json(patch): Json<ReportSourcePatch>,
) -> ApiResult {
    Ok(Json(json!(
        repo::set_report_source(&s.pool, &source, patch).await?
    )))
}

// --- the board rollup (#970) ------------------------------------------------

/// The whole board in one request: active sprints with progress, the ranked
/// queue, programs with their slices, the awaiting-Ken lane, per-project depth
/// and the newest reports. Takes no query parameters — see `get_board`'s tool
/// description and `repo::board_rollup` for why.
async fn board(State(s): State<AppState>) -> ApiResult {
    Ok(Json(json!(repo::board_rollup(&s.pool).await?)))
}

// --- the awaiting-Ken marker (#969) ----------------------------------------

/// The Commander's Call lane, oldest ask first. Ghost-free by D-7.
async fn list_awaiting(State(s): State<AppState>) -> ApiResult {
    Ok(Json(json!(repo::list_awaiting(&s.pool).await?)))
}

/// Set or clear the marker on any node. The web UI's one-click clear is this
/// call with `awaiting: false` — the same core path an agent uses, not a second
/// one that could drift from it.
async fn set_awaiting(
    State(s): State<AppState>,
    Path(node_id): Path<i64>,
    Json(b): Json<ops::SetAwaiting>,
) -> ApiResult {
    Ok(Json(json!(
        repo::set_awaiting(&s.pool, node_id, b.awaiting, b.note.as_deref()).await?
    )))
}

// --- handoffs (durable cross-agent context) --------------------------------

async fn create_handoff(State(s): State<AppState>, Json(b): Json<NewHandoff>) -> ApiResult {
    Ok(Json(json!(repo::create_handoff(&s.pool, b).await?)))
}

/// The authoritative "read this handoff" call — body plus the nodes it belongs
/// to. A `has_handoff` edge in a work-item/proposal `related` block points here.
async fn get_handoff(State(s): State<AppState>, Path(node_id): Path<i64>) -> ApiResult {
    match repo::get_handoff(&s.pool, node_id).await? {
        Some(full) => Ok(Json(json!(full))),
        None => Err(not_found(format!("no handoff with node_id {node_id}"))),
    }
}

async fn update_handoff(
    State(s): State<AppState>,
    Path(node_id): Path<i64>,
    Json(patch): Json<HandoffPatch>,
) -> ApiResult {
    Ok(Json(json!(
        repo::update_handoff(&s.pool, node_id, patch).await?
    )))
}

#[cfg(test)]
mod spa_tests {
    use super::*;
    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use axum::routing::get;
    use http_body_util::BodyExt;
    use tower::ServiceExt;

    // WI #284 regression: deep links must serve the SPA shell with a 200, not a
    // 404. Exercises spa_fallback directly so it needs neither a DB nor env vars.
    #[tokio::test]
    async fn deep_links_serve_shell_with_200() {
        let dir = std::env::temp_dir().join(format!("korg-spa-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("index.html"),
            "<!doctype html><title>KORG-SHELL</title>",
        )
        .unwrap();
        std::fs::write(dir.join("favicon.png"), b"realbytes").unwrap();

        let api = Router::new().route("/api/health", get(|| async { "ok" }));
        let router = spa_fallback(api, &dir);

        let hit = |path: &'static str| {
            let router = router.clone();
            async move {
                let resp = router
                    .oneshot(Request::builder().uri(path).body(Body::empty()).unwrap())
                    .await
                    .unwrap();
                let status = resp.status();
                let body = resp.into_body().collect().await.unwrap().to_bytes();
                (status, String::from_utf8_lossy(&body).into_owned())
            }
        };

        // Client-side routes fall back to the shell — with a 200, the whole point.
        for path in ["/plan", "/planning", "/work-items"] {
            let (status, body) = hit(path).await;
            assert_eq!(status, StatusCode::OK, "{path} should be 200");
            assert!(body.contains("KORG-SHELL"), "{path} should serve the shell");
        }

        // Real files are served from disk, not the shell.
        let (status, body) = hit("/favicon.png").await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body, "realbytes");

        // API routes still win over the fallback.
        let (status, body) = hit("/api/health").await;
        assert_eq!(status, StatusCode::OK);
        assert!(!body.contains("KORG-SHELL"));

        std::fs::remove_dir_all(&dir).ok();
    }
}
