//! korg-api — axum REST API over korg-core, and static host for the web bundle.

use std::path::PathBuf;
use std::sync::Arc;

use axum::extract::{Path, Query, State};
use axum::http::{HeaderValue, Method};
use axum::routing::{delete, get, patch, post};
use axum::{Json, Router};
use rust_decimal::Decimal;
use serde::{Deserialize, Deserializer};
use serde_json::{json, Value};
use sqlx::PgPool;
use time::macros::format_description;
use time::Date;
use tower_http::cors::CorsLayer;
use tower_http::services::{ServeDir, ServeFile};
use tower_http::trace::TraceLayer;

use korg_core::repo::{
    self, CardPatch, NewCard, NewLink, NewProposal, NewWorkItem, ProposalPatch,
};
use korg_core::slots::{self, NewTemplateSlot};
use korg_mcp::tools::KorgServer;
use rmcp::transport::streamable_http_server::session::local::LocalSessionManager;
use rmcp::transport::streamable_http_server::{StreamableHttpServerConfig, StreamableHttpService};

pub mod error;
use error::ApiError;

#[derive(Clone)]
pub struct AppState {
    pub pool: Arc<PgPool>,
}

type ApiResult = Result<Json<Value>, ApiError>;

pub fn build_router(state: AppState) -> Router {
    let mcp = mcp_service(state.pool.clone());
    let api = Router::new()
        .route("/api/health", get(health))
        .route("/api/projects", get(list_projects).post(create_project))
        .route("/api/projects/recent", get(recent_project))
        .route("/api/work-items", get(list_work_items).post(create_work_item))
        .route("/api/work-items/survey", get(survey_work_items))
        .route("/api/work-items/:wi_number", get(get_work_item).patch(update_work_item))
        .route("/api/areas", get(list_areas).post(create_area))
        .route("/api/cards", get(list_cards).post(create_card))
        .route("/api/cards/:node_id", patch(update_card))
        .route("/api/nodes/:id", get(get_node))
        .route("/api/nodes/:node_id/comments", get(list_comments).post(add_comment))
        .route("/api/comments/:id", delete(delete_comment))
        .route("/api/links", get(list_links).post(create_link))
        .route("/api/links/:node_id", patch(update_link))
        .route("/api/slots", get(list_slots))
        .route("/api/slots/generate", post(generate_slots))
        .route("/api/slots/:node_id", patch(update_slot))
        .route("/api/slot-templates", get(list_slot_templates).put(set_slot_templates))
        .route("/api/relationships", post(create_relationship))
        .route("/api/relationships/:id", delete(delete_relationship))
        .route("/api/nodes/:id/neighbors", get(neighbors))
        .route("/api/projects/:name/plan", get(project_plan))
        .route("/api/proposals", get(list_proposals).post(create_proposal))
        .route("/api/reports", get(list_reports))
        .route("/api/reports/:node_id", get(get_report))
        .route("/api/proposals/:node_id", patch(update_proposal))
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
) -> StreamableHttpService<KorgServer, LocalSessionManager> {
    let config = StreamableHttpServerConfig::default()
        .with_stateful_mode(false)
        .with_json_response(true)
        .disable_allowed_hosts();
    StreamableHttpService::new(
        move || Ok(KorgServer::new((*pool).clone())),
        Arc::new(LocalSessionManager::default()),
        config,
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
    Date::parse(s, &fmt).map_err(|e| ApiError(anyhow::anyhow!("invalid date `{s}`: {e}")))
}

async fn health() -> Json<Value> {
    Json(json!({ "status": "ok" }))
}

// --- projects -------------------------------------------------------------

async fn list_projects(State(s): State<AppState>) -> ApiResult {
    Ok(Json(json!(repo::list_projects(&s.pool).await?)))
}

#[derive(Deserialize)]
struct CreateProject {
    name: String,
}

async fn create_project(State(s): State<AppState>, Json(b): Json<CreateProject>) -> ApiResult {
    let id = repo::create_project(&s.pool, &b.name).await?;
    Ok(Json(json!({ "id": id, "name": b.name })))
}

async fn recent_project(State(s): State<AppState>) -> ApiResult {
    Ok(Json(json!({ "project": repo::recent_project(&s.pool).await? })))
}

// --- work items -----------------------------------------------------------

#[derive(Deserialize)]
struct WorkItemsQuery {
    project: Option<String>,
}

async fn list_work_items(State(s): State<AppState>, Query(q): Query<WorkItemsQuery>) -> ApiResult {
    let items = match q.project {
        Some(p) => repo::list_work_items_by_project(&s.pool, &p).await?,
        None => repo::list_work_items(&s.pool).await?,
    };
    Ok(Json(json!(items)))
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

async fn get_work_item(State(s): State<AppState>, Path(wi): Path<i64>) -> ApiResult {
    Ok(Json(json!(repo::get_work_item(&s.pool, wi).await?)))
}

#[derive(Deserialize)]
struct CreateWorkItem {
    title: String,
    content: String,
    #[serde(default = "d_task")]
    wi_type: String,
    #[serde(default = "d_open")]
    wi_status: String,
    #[serde(default = "d_unknown")]
    wi_tshirt: String,
    #[serde(default)]
    sprint: Option<String>,
    #[serde(default)]
    details: Option<String>,
    #[serde(default)]
    project_id: Option<i64>,
    #[serde(default)]
    area_id: Option<i64>,
    #[serde(default)]
    category: Option<String>,
    #[serde(default)]
    tags: Vec<String>,
}
fn d_task() -> String { "task".into() }
fn d_open() -> String { "open".into() }
fn d_unknown() -> String { "Unknown".into() }
fn d_backlog() -> String { "Backlog".into() }

async fn create_work_item(State(s): State<AppState>, Json(b): Json<CreateWorkItem>) -> ApiResult {
    let r = repo::create_work_item(
        &s.pool,
        NewWorkItem {
            project_id: b.project_id,
            area_id: b.area_id,
            wi_type: b.wi_type,
            wi_status: b.wi_status,
            wi_tshirt: b.wi_tshirt,
            sprint: b.sprint,
            title: b.title,
            content: b.content,
            details: b.details,
            category: b.category,
            tags: b.tags,
        },
    )
    .await?;
    Ok(Json(json!({ "node_id": r.node_id, "wi_number": r.wi_number })))
}

fn deser_nullable_str<'de, D>(d: D) -> Result<Option<Option<String>>, D::Error>
where
    D: Deserializer<'de>,
{
    Option::<String>::deserialize(d).map(Some)
}

fn deser_nullable_i64<'de, D>(d: D) -> Result<Option<Option<i64>>, D::Error>
where
    D: Deserializer<'de>,
{
    Option::<i64>::deserialize(d).map(Some)
}

#[derive(Deserialize)]
struct UpdateWorkItem {
    #[serde(default)]
    title: Option<String>,
    #[serde(default)]
    content: Option<String>,
    #[serde(default, deserialize_with = "deser_nullable_str")]
    details: Option<Option<String>>,
    #[serde(default)]
    wi_type: Option<String>,
    #[serde(default)]
    wi_status: Option<String>,
    #[serde(default)]
    wi_tshirt: Option<String>,
    #[serde(default, deserialize_with = "deser_nullable_str")]
    sprint: Option<Option<String>>,
    #[serde(default, deserialize_with = "deser_nullable_i64")]
    area_id: Option<Option<i64>>,
    #[serde(default, deserialize_with = "deser_nullable_i64")]
    parent: Option<Option<i64>>,
    #[serde(default)]
    archived: Option<bool>,
    #[serde(default)]
    tags: Option<Vec<String>>,
}

async fn update_work_item(
    State(s): State<AppState>,
    Path(wi): Path<i64>,
    Json(b): Json<UpdateWorkItem>,
) -> ApiResult {
    repo::update_work_item(
        &s.pool,
        wi,
        repo::WorkItemPatch {
            title: b.title,
            content: b.content,
            details: b.details,
            wi_type: b.wi_type,
            wi_status: b.wi_status,
            wi_tshirt: b.wi_tshirt,
            sprint: b.sprint,
            area_id: b.area_id,
            parent: b.parent,
            archived: b.archived,
            category: None,
            tags: b.tags,
        },
    )
    .await?;
    Ok(Json(json!({ "ok": true })))
}

#[derive(Deserialize)]
struct AreasQuery {
    project: String,
}

async fn list_areas(State(s): State<AppState>, Query(q): Query<AreasQuery>) -> ApiResult {
    Ok(Json(json!(repo::list_areas(&s.pool, &q.project).await?)))
}

#[derive(Deserialize)]
struct CreateArea {
    project: String,
    name: String,
    #[serde(default)]
    description: Option<String>,
}

async fn create_area(State(s): State<AppState>, Json(b): Json<CreateArea>) -> ApiResult {
    let id = repo::create_area(&s.pool, &b.project, &b.name, b.description.as_deref()).await?;
    Ok(Json(json!({ "id": id, "name": b.name })))
}

// --- cards ----------------------------------------------------------------

async fn list_cards(State(s): State<AppState>) -> ApiResult {
    Ok(Json(json!(repo::list_cards(&s.pool).await?)))
}

#[derive(Deserialize)]
struct CreateCard {
    title: String,
    #[serde(default = "d_backlog")]
    status: String,
    #[serde(default)]
    description: String,
    #[serde(default)]
    rank: f64,
    #[serde(default)]
    project_id: Option<i64>,
    #[serde(default)]
    category: Option<String>,
    #[serde(default)]
    tags: Vec<String>,
}

async fn create_card(State(s): State<AppState>, Json(b): Json<CreateCard>) -> ApiResult {
    let rank = Decimal::try_from(b.rank).map_err(|e| ApiError(anyhow::anyhow!("rank: {e}")))?;
    let node_id = repo::create_card(
        &s.pool,
        NewCard {
            project_id: b.project_id,
            category: b.category,
            tags: b.tags,
            status: b.status,
            title: b.title,
            description: b.description,
            rank,
        },
    )
    .await?;
    Ok(Json(json!({ "node_id": node_id })))
}

#[derive(Deserialize)]
struct UpdateCard {
    #[serde(default)]
    status: Option<String>,
    #[serde(default)]
    rank: Option<f64>,
    #[serde(default)]
    title: Option<String>,
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    archived: Option<bool>,
    #[serde(default, deserialize_with = "deser_nullable_str")]
    project: Option<Option<String>>,
    #[serde(default, deserialize_with = "deser_nullable_str")]
    category: Option<Option<String>>,
    #[serde(default)]
    tags: Option<Vec<String>>,
}

async fn update_card(
    State(s): State<AppState>,
    Path(node_id): Path<i64>,
    Json(b): Json<UpdateCard>,
) -> ApiResult {
    let rank = match b.rank {
        Some(r) => Some(Decimal::try_from(r).map_err(|e| ApiError(anyhow::anyhow!("rank: {e}")))?),
        None => None,
    };
    // Resolve a free-text project name to a project id (creating it if needed).
    let project_id: Option<Option<i64>> = match &b.project {
        Some(Some(name)) if !name.trim().is_empty() => {
            Some(Some(repo::create_project(&s.pool, name.trim()).await?))
        }
        Some(_) => Some(None),
        None => None,
    };
    repo::update_card(
        &s.pool,
        node_id,
        CardPatch {
            status: b.status,
            rank,
            title: b.title,
            description: b.description,
            archived: b.archived,
            project_id,
            category: b.category,
            tags: b.tags,
        },
    )
    .await?;
    Ok(Json(json!({ "ok": true })))
}

async fn list_comments(State(s): State<AppState>, Path(node_id): Path<i64>) -> ApiResult {
    Ok(Json(json!(repo::list_comments(&s.pool, node_id).await?)))
}

#[derive(Deserialize)]
struct NewComment {
    body: String,
}

async fn add_comment(
    State(s): State<AppState>,
    Path(node_id): Path<i64>,
    Json(b): Json<NewComment>,
) -> ApiResult {
    Ok(Json(json!(repo::add_comment(&s.pool, node_id, &b.body).await?)))
}

async fn delete_comment(State(s): State<AppState>, Path(id): Path<i64>) -> ApiResult {
    repo::delete_comment(&s.pool, id).await?;
    Ok(Json(json!({ "ok": true })))
}

// --- links (reading list) -------------------------------------------------

async fn list_links(State(s): State<AppState>) -> ApiResult {
    Ok(Json(json!(repo::list_links(&s.pool).await?)))
}

#[derive(Deserialize)]
struct CreateLink {
    url: String,
    #[serde(default)]
    title: Option<String>,
    #[serde(default)]
    project_id: Option<i64>,
    #[serde(default)]
    category: Option<String>,
    #[serde(default)]
    tags: Vec<String>,
}

async fn create_link(State(s): State<AppState>, Json(b): Json<CreateLink>) -> ApiResult {
    let node_id = repo::create_link(
        &s.pool,
        NewLink {
            project_id: b.project_id,
            category: b.category,
            tags: b.tags,
            url: b.url,
            title: b.title,
        },
    )
    .await?;
    Ok(Json(json!({ "node_id": node_id })))
}

#[derive(Deserialize)]
struct UpdateLink {
    #[serde(default)]
    disposition: Option<String>,
    #[serde(default)]
    read: Option<bool>,
    #[serde(default)]
    tags: Option<Vec<String>>,
}

async fn update_link(
    State(s): State<AppState>,
    Path(node_id): Path<i64>,
    Json(b): Json<UpdateLink>,
) -> ApiResult {
    if let Some(d) = &b.disposition {
        repo::set_link_disposition(&s.pool, node_id, d).await?;
    }
    if let Some(r) = b.read {
        repo::mark_link_read(&s.pool, node_id, r).await?;
    }
    if let Some(t) = &b.tags {
        repo::set_node_tags(&s.pool, node_id, t).await?;
    }
    Ok(Json(json!({ "ok": true })))
}

// --- slots ----------------------------------------------------------------

#[derive(Deserialize)]
struct SlotRange {
    from: String,
    to: String,
}

async fn list_slots(State(s): State<AppState>, Query(q): Query<SlotRange>) -> ApiResult {
    let (from, to) = (parse_date(&q.from)?, parse_date(&q.to)?);
    Ok(Json(json!(slots::list_slots(&s.pool, from, to).await?)))
}

#[derive(Deserialize)]
struct GenerateSlots {
    start: String,
    days: i64,
}

async fn generate_slots(State(s): State<AppState>, Json(b): Json<GenerateSlots>) -> ApiResult {
    let start = parse_date(&b.start)?;
    let created = slots::generate_slots(&s.pool, start, b.days).await?;
    Ok(Json(json!({ "created": created })))
}

#[derive(Deserialize)]
struct UpdateSlot {
    #[serde(default)]
    goal: Option<String>,
}

async fn update_slot(
    State(s): State<AppState>,
    Path(node_id): Path<i64>,
    Json(b): Json<UpdateSlot>,
) -> ApiResult {
    slots::set_slot_goal(&s.pool, node_id, b.goal.as_deref()).await?;
    Ok(Json(json!({ "ok": true })))
}

async fn list_slot_templates(State(s): State<AppState>) -> ApiResult {
    Ok(Json(json!(slots::list_templates(&s.pool).await?)))
}

#[derive(Deserialize)]
struct TemplateRow {
    dow: i16,
    position: i32,
    duration_minutes: i32,
    #[serde(default)]
    label: Option<String>,
}

#[derive(Deserialize)]
struct SetTemplates {
    slots: Vec<TemplateRow>,
}

async fn set_slot_templates(State(s): State<AppState>, Json(b): Json<SetTemplates>) -> ApiResult {
    let rows: Vec<NewTemplateSlot> = b
        .slots
        .into_iter()
        .map(|t| NewTemplateSlot {
            dow: t.dow,
            position: t.position,
            duration_minutes: t.duration_minutes,
            label: t.label,
        })
        .collect();
    slots::set_weekly_template(&s.pool, &rows).await?;
    Ok(Json(json!({ "ok": true })))
}

// --- relationships --------------------------------------------------------

#[derive(Deserialize)]
struct CreateRelationship {
    left: i64,
    right: i64,
    label: String,
}

async fn create_relationship(
    State(s): State<AppState>,
    Json(b): Json<CreateRelationship>,
) -> ApiResult {
    let id = repo::relate(&s.pool, b.left, b.right, &b.label).await?;
    Ok(Json(json!({ "id": id })))
}

async fn delete_relationship(State(s): State<AppState>, Path(id): Path<i64>) -> ApiResult {
    repo::unrelate(&s.pool, id).await?;
    Ok(Json(json!({ "ok": true })))
}

async fn neighbors(State(s): State<AppState>, Path(id): Path<i64>) -> ApiResult {
    Ok(Json(json!(repo::neighbors(&s.pool, id).await?)))
}

/// Kind-agnostic preview of any node by its id (WI #260). Returns `null` when
/// no node has that id, so the find-by-ID box can say "not found" cleanly.
async fn get_node(State(s): State<AppState>, Path(id): Path<i64>) -> ApiResult {
    Ok(Json(json!(repo::get_node_preview(&s.pool, id).await?)))
}

/// Plan view payload: a project's work items plus its `depends_on` edges
/// ([left, right] = left depends on right). Frontier/blocked computation
/// happens client-side — the full item set is already in the payload.
async fn project_plan(State(s): State<AppState>, Path(name): Path<String>) -> ApiResult {
    let items = repo::list_work_items_by_project(&s.pool, &name).await?;
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
        None => Err(ApiError(anyhow::anyhow!("no report with node_id {node_id}"))),
    }
}

// --- sprint proposals (agent planning) -------------------------------------

#[derive(Deserialize)]
struct ProposalsQuery {
    status: Option<String>,
}

async fn list_proposals(State(s): State<AppState>, Query(q): Query<ProposalsQuery>) -> ApiResult {
    Ok(Json(json!(
        repo::list_proposals(&s.pool, q.status.as_deref()).await?
    )))
}

#[derive(Deserialize)]
struct CreateProposal {
    title: String,
    summary: String,
    #[serde(default)]
    work_item_numbers: Vec<i64>,
    #[serde(default)]
    project_id: Option<i64>,
    #[serde(default)]
    rank: f64,
    #[serde(default)]
    pinned: bool,
    #[serde(default)]
    category: Option<String>,
    #[serde(default)]
    tags: Vec<String>,
}

async fn create_proposal(State(s): State<AppState>, Json(b): Json<CreateProposal>) -> ApiResult {
    let rank = Decimal::try_from(b.rank).map_err(|e| ApiError(anyhow::anyhow!("rank: {e}")))?;
    let r = repo::create_proposal(
        &s.pool,
        NewProposal {
            project_id: b.project_id,
            category: b.category,
            tags: b.tags,
            title: b.title,
            summary: b.summary,
            rank,
            pinned: b.pinned,
            covers: b.work_item_numbers,
        },
    )
    .await?;
    Ok(Json(json!({ "node_id": r.node_id, "covered": r.covered })))
}

#[derive(Deserialize)]
struct UpdateProposal {
    #[serde(default)]
    title: Option<String>,
    #[serde(default)]
    summary: Option<String>,
    #[serde(default)]
    status: Option<String>,
    #[serde(default)]
    rank: Option<f64>,
    #[serde(default)]
    pinned: Option<bool>,
    #[serde(default)]
    archived: Option<bool>,
    #[serde(default)]
    tags: Option<Vec<String>>,
}

async fn update_proposal(
    State(s): State<AppState>,
    Path(node_id): Path<i64>,
    Json(b): Json<UpdateProposal>,
) -> ApiResult {
    let rank = match b.rank {
        Some(r) => Some(Decimal::try_from(r).map_err(|e| ApiError(anyhow::anyhow!("rank: {e}")))?),
        None => None,
    };
    repo::update_proposal(
        &s.pool,
        node_id,
        ProposalPatch {
            title: b.title,
            summary: b.summary,
            status: b.status,
            rank,
            pinned: b.pinned,
            archived: b.archived,
            tags: b.tags,
        },
    )
    .await?;
    Ok(Json(json!({ "ok": true })))
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
        std::fs::write(dir.join("index.html"), "<!doctype html><title>KORG-SHELL</title>").unwrap();
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
