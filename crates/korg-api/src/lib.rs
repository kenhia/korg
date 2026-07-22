//! korg-api — axum REST API over korg-core, and static host for the web bundle.

use std::path::PathBuf;
use std::sync::Arc;

use axum::extract::{Path, Query, State};
use axum::http::{HeaderValue, Method};
use axum::routing::{delete, get, patch, post, put};
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::{json, Value};
use sqlx::PgPool;
use time::macros::format_description;
use time::{Date, Duration};
use tower_http::cors::CorsLayer;
use tower_http::services::{ServeDir, ServeFile};
use tower_http::trace::TraceLayer;

use korg_core::config::KorgConfig;
use korg_core::ops;
use korg_core::repo::{
    self, CardPatch, LinkPatch, NewCard, NewLink, NewProposal, NewWorkItem, ProjectPatch,
    ProposalPatch, WorkItemPatch,
};
use korg_core::{daily_plan, topics};
use korg_mcp::tools::KorgServer;
use rmcp::transport::streamable_http_server::session::local::LocalSessionManager;
use rmcp::transport::streamable_http_server::{StreamableHttpServerConfig, StreamableHttpService};

pub mod error;
use error::ApiError;

#[derive(Clone)]
pub struct AppState {
    pub pool: Arc<PgPool>,
    pub config: Arc<KorgConfig>,
}

type ApiResult = Result<Json<Value>, ApiError>;

pub fn build_router(state: AppState) -> Router {
    let mcp = mcp_service(state.pool.clone(), state.config.clone());
    let api = Router::new()
        .route("/api/health", get(health))
        .route("/api/projects", get(list_projects).post(create_project))
        .route("/api/projects/recent", get(recent_project))
        .route(
            "/api/work-items",
            get(list_work_items).post(create_work_item),
        )
        .route("/api/work-items/survey", get(survey_work_items))
        .route(
            "/api/work-items/:wi_number",
            get(get_work_item).patch(update_work_item),
        )
        .route("/api/areas", get(list_areas).post(create_area))
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
        .route("/api/links/:node_id", patch(update_link))
        .route("/api/topics", get(list_topics).post(create_topic))
        .route("/api/topics/:node_id", get(get_topic).patch(update_topic))
        .route("/api/topics/:node_id/archive", post(archive_topic))
        .route(
            "/api/daily-plan",
            get(list_daily_plan).post(create_daily_plan_item),
        )
        .route("/api/daily-plan/history", get(daily_plan_history))
        .route("/api/daily-plan/:node_id", delete(delete_daily_plan_item))
        .route(
            "/api/daily-plan/:node_id/completion",
            patch(set_daily_plan_completion),
        )
        .route("/api/daily-plan/:node_id/move", post(move_daily_plan_item))
        .route("/api/daily-plan/:plan_date/order", put(reorder_daily_plan))
        .route("/api/relationships", post(create_relationship))
        .route("/api/relationships/:id", delete(delete_relationship))
        .route("/api/nodes/:id/neighbors", get(neighbors))
        .route("/api/projects/:name/plan", get(project_plan))
        .route("/api/projects/:name", patch(update_project))
        .route("/api/proposals", get(list_proposals).post(create_proposal))
        .route("/api/reports", get(list_reports))
        .route("/api/reports/:node_id", get(get_report))
        .route(
            "/api/proposals/:node_id",
            get(get_proposal).patch(update_proposal),
        )
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
/// upstream 404 onto it, so deep links / bookmarks (e.g. /plan) load the page
/// with a 404 status. `fallback` preserves the shell's 200.
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
    config: Arc<KorgConfig>,
) -> StreamableHttpService<KorgServer, LocalSessionManager> {
    let transport_config = StreamableHttpServerConfig::default()
        .with_stateful_mode(false)
        .with_json_response(true)
        .disable_allowed_hosts();
    StreamableHttpService::new(
        move || Ok(KorgServer::new((*pool).clone(), config.clone())),
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

fn parse_date(s: &str) -> Result<Date, ApiError> {
    let fmt = format_description!("[year]-[month]-[day]");
    Date::parse(s, &fmt).map_err(|e| ApiError::invalid(format!("invalid date `{s}`: {e}")))
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
    archived: Option<bool>,
    limit: Option<i64>,
    offset: Option<i64>,
}

async fn survey_work_items(State(s): State<AppState>, Query(q): Query<SurveyQuery>) -> ApiResult {
    let limit = q.limit.unwrap_or(50).clamp(1, 500);
    let offset = q.offset.unwrap_or(0).max(0);
    let survey = repo::survey_work_items(
        &s.pool,
        q.project.as_deref(),
        q.wi_status.as_deref(),
        q.archived,
        limit,
        offset,
    )
    .await?;
    Ok(Json(json!(survey)))
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

// --- topics and daily planning --------------------------------------------

#[derive(Deserialize)]
struct TopicsQuery {
    #[serde(default)]
    q: Option<String>,
    archived: Option<String>,
    limit: Option<i64>,
    offset: Option<i64>,
}

async fn list_topics(State(s): State<AppState>, Query(q): Query<TopicsQuery>) -> ApiResult {
    let page = topics::list_topics(
        &s.pool,
        topics::TopicQuery {
            q: q.q,
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

async fn create_topic(State(s): State<AppState>, Json(b): Json<topics::NewTopic>) -> ApiResult {
    Ok(Json(json!(topics::create_topic(&s.pool, b).await?)))
}

async fn get_topic(State(s): State<AppState>, Path(node_id): Path<i64>) -> ApiResult {
    match topics::get_topic(&s.pool, node_id).await? {
        Some(topic) => Ok(Json(json!(topic))),
        None => Err(not_found(format!("no topic with node_id {node_id}"))),
    }
}

async fn update_topic(
    State(s): State<AppState>,
    Path(node_id): Path<i64>,
    Json(patch): Json<topics::TopicPatch>,
) -> ApiResult {
    Ok(Json(json!(
        topics::update_topic(&s.pool, node_id, patch).await?
    )))
}

async fn archive_topic(
    State(s): State<AppState>,
    Path(node_id): Path<i64>,
    Json(b): Json<ops::ArchiveTopic>,
) -> ApiResult {
    let topic = topics::archive_topic(&s.pool, node_id, b.archived).await?;
    Ok(Json(json!(topic)))
}

async fn list_daily_plan(State(s): State<AppState>, Query(q): Query<ops::DateRange>) -> ApiResult {
    Ok(Json(json!(
        daily_plan::list_items(&s.pool, parse_date(&q.from)?, parse_date(&q.to)?,).await?
    )))
}

async fn create_daily_plan_item(
    State(s): State<AppState>,
    Json(b): Json<ops::CreateDailyPlanItem>,
) -> ApiResult {
    let context = s.config.lifecycle_context()?;
    let item = daily_plan::create_item(
        &s.pool,
        b.source_node_id,
        parse_date(&b.plan_date)?,
        &context,
    )
    .await?;
    Ok(Json(json!(item)))
}

async fn set_daily_plan_completion(
    State(s): State<AppState>,
    Path(node_id): Path<i64>,
    Json(b): Json<ops::SetCompletion>,
) -> ApiResult {
    let item = daily_plan::set_completion(
        &s.pool,
        node_id,
        b.completed,
        &s.config.lifecycle_context()?,
    )
    .await?;
    Ok(Json(json!(item)))
}

async fn delete_daily_plan_item(State(s): State<AppState>, Path(node_id): Path<i64>) -> ApiResult {
    daily_plan::delete_item(&s.pool, node_id, &s.config.lifecycle_context()?).await?;
    Ok(Json(json!({ "deleted": true })))
}

async fn reorder_daily_plan(
    State(s): State<AppState>,
    Path(plan_date): Path<String>,
    Json(b): Json<ops::ReorderDailyPlan>,
) -> ApiResult {
    let items = daily_plan::reorder_day(
        &s.pool,
        parse_date(&plan_date)?,
        &b.node_ids,
        &s.config.lifecycle_context()?,
    )
    .await?;
    Ok(Json(json!(items)))
}

async fn move_daily_plan_item(
    State(s): State<AppState>,
    Path(node_id): Path<i64>,
    Json(b): Json<ops::MoveDailyPlanItem>,
) -> ApiResult {
    Ok(Json(json!(
        daily_plan::move_item(
            &s.pool,
            node_id,
            parse_date(&b.target_date)?,
            b.target_position,
            &s.config.lifecycle_context()?,
        )
        .await?
    )))
}

#[derive(Deserialize)]
struct HistoryQuery {
    #[serde(default)]
    from: Option<String>,
    #[serde(default)]
    to: Option<String>,
    #[serde(default)]
    preset: Option<String>,
    #[serde(default)]
    source_node_id: Option<i64>,
}

async fn daily_plan_history(State(s): State<AppState>, Query(q): Query<HistoryQuery>) -> ApiResult {
    let context = s.config.lifecycle_context()?;
    let (from, to) = match q.preset.as_deref() {
        Some(preset) => {
            let days = match preset {
                "week" => 7,
                "month" => 30,
                "90days" => 90,
                "year" => 365,
                _ => return Err(ApiError::invalid("invalid history preset")),
            };
            let to = context.today - Duration::days(1);
            (to - Duration::days(days - 1), to)
        }
        None => {
            let from = q
                .from
                .as_deref()
                .ok_or_else(|| ApiError::invalid("from is required without preset"))?;
            let to =
                q.to.as_deref()
                    .ok_or_else(|| ApiError::invalid("to is required without preset"))?;
            (parse_date(from)?, parse_date(to)?)
        }
    };
    Ok(Json(json!(
        daily_plan::history(&s.pool, from, to, q.source_node_id, &context,).await?
    )))
}

// --- relationships --------------------------------------------------------

async fn create_relationship(State(s): State<AppState>, Json(b): Json<ops::Relate>) -> ApiResult {
    let id = repo::relate(&s.pool, b.left, b.right, &b.label).await?;
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
async fn project_plan(State(s): State<AppState>, Path(name): Path<String>) -> ApiResult {
    let items = repo::list_work_items(
        &s.pool,
        repo::WorkItemQuery {
            project: Some(name.clone()),
            archived: None,
            page: repo::PageQuery {
                limit: Some(repo::LIST_LIMIT_MAX),
                offset: None,
            },
        },
    )
    .await?
    .items;
    let edges = repo::project_edges(&s.pool, &name, "depends_on").await?;
    Ok(Json(json!({ "items": items, "edges": edges })))
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
