//! Image bytes over REST (#1119; handoff `korg:1128` D6).
//!
//! **Bytes go over REST, metadata goes over MCP.** One multipart upload
//! endpoint serves the web UI and agents alike — `curl -F` from kai is a
//! first-class client, not an afterthought — and the serve routes address an
//! image by its display id with an optional variant. Nothing here is
//! base64-over-MCP: an agent fetches the `agent` variant to a temp file and
//! reads it natively, which costs one HTTP request instead of inflating the
//! bytes by a third through a JSON-RPC channel.
//!
//! The routes themselves are registered in `lib.rs` with every other route.
//! That is deliberate and not merely tidiness: `docs_drift::the_rest_table_
//! matches_the_router` scans `lib.rs` for `.route(` calls, so a route hidden in
//! a submodule would be a route the documentation fence cannot see.
//!
//! Decoding and disk I/O are blocking work — resizing a 4K screenshot twice is
//! tens of milliseconds of CPU — so both happen inside `spawn_blocking` rather
//! than on the runtime's worker threads.

use axum::body::Bytes;
use axum::extract::{Multipart, Path, Query, State};
use axum::http::{header, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde::Deserialize;
use serde_json::json;

use korg_core::repo::{self, NewAttachment, ATTACHMENT_PENDING_TTL_HOURS};
use korg_img::{ImgId, Store, Variant};

use crate::error::ApiError;
use crate::{ApiResult, AppState};

/// How often the background sweeper runs. Not configurable: the interval only
/// has to be small relative to [`ATTACHMENT_PENDING_TTL_HOURS`], and a knob
/// nobody would turn is a knob that ends up documented, tested and wrong.
pub const SWEEP_INTERVAL_HOURS: u64 = 1;

// --- upload -----------------------------------------------------------------

#[derive(Debug, Default, Deserialize)]
pub struct UploadQuery {
    /// The node this image belongs to. Omit for paste-before-save: the
    /// attachment is created `pending` and is swept if nothing claims it.
    ///
    /// A query parameter rather than a multipart field because the body **is**
    /// the image and the target is addressing — and because it keeps the
    /// agent-facing call to one `-F`.
    pub owner: Option<i64>,
}

/// `POST /api/img[?owner=<node_id>]` — multipart upload, returning the created
/// attachment's metadata (including its `img-<hex>` id and variant URLs).
///
/// The declared `Content-Type` of the part is deliberately ignored: the type
/// korg records is sniffed from the bytes, because a claim about a file is not
/// a file. The upload cap is enforced twice — by the route's body limit before
/// the bytes are buffered, and by `korg_img::prepare` after — since the first
/// is a transport concern that a future router change could drop.
pub async fn upload(
    State(s): State<AppState>,
    Query(q): Query<UploadQuery>,
    mut multipart: Multipart,
) -> ApiResult {
    let mut part: Option<(String, Bytes)> = None;
    while let Some(field) = multipart
        .next_field()
        .await
        .map_err(|e| ApiError::invalid(format!("malformed multipart upload: {e}")))?
    {
        // The first part carrying a filename is the image. Named `file` by
        // convention, but not required to be: `curl -F image=@shot.png` and a
        // browser's FormData both arrive here and neither should have to guess
        // korg's field name.
        let Some(filename) = field.file_name().map(str::to_owned) else {
            continue;
        };
        let bytes = field
            .bytes()
            .await
            .map_err(|e| ApiError::invalid(format!("could not read uploaded file: {e}")))?;
        part = Some((filename, bytes));
        break;
    }
    let (filename, bytes) = part.ok_or_else(|| {
        ApiError::invalid(
            "no file in the upload — send it as a multipart part with a filename, \
             e.g. `curl -F file=@shot.png`",
        )
    })?;

    // Decode and generate both variants off the runtime's worker threads.
    let (bytes, prepared) = tokio::task::spawn_blocking(move || {
        korg_img::prepare(&bytes).map(|prepared| (bytes, prepared))
    })
    .await??;

    let new = NewAttachment::from_prepared(sanitize_filename(&filename), &prepared, q.owner);
    let row = repo::create_attachment(&s.pool, new).await?;
    let id = ImgId::from_node_id(row.node_id)?;

    // Blobs after the row, and the row is compensated if they fail — see the
    // attachment module's comment in korg-core for why this order. Even if the
    // compensating delete fails, the record has no owner edge, so the sweeper
    // collects it rather than leaving a permanent broken image.
    let store = s.images.clone();
    let write = tokio::task::spawn_blocking(move || store.write(id, &prepared, &bytes)).await?;
    if let Err(e) = write {
        let removed = repo::delete_attachment(&s.pool, row.node_id).await;
        tracing::error!(
            img_id = %id, error = %e, compensated = ?removed,
            "image store write failed; attachment record rolled back"
        );
        return Err(ApiError(e.into()));
    }

    tracing::info!(
        img_id = %id, owner = ?q.owner, bytes = row.byte_size,
        "stored image"
    );
    Ok(Json(json!(row)))
}

/// Keep a name that is safe to render and safe to log. The store never uses
/// this to build a path — blobs are named after the variant, and the directory
/// after the id — so this is presentation hygiene rather than a security
/// boundary; the boundary is that the filename is never a path component.
fn sanitize_filename(raw: &str) -> String {
    let base = raw
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or(raw)
        .trim()
        .trim_matches('.');
    let cleaned: String = base
        .chars()
        .filter(|c| !c.is_control())
        .take(200)
        .collect::<String>()
        .trim()
        .to_string();
    if cleaned.is_empty() {
        "upload".to_string()
    } else {
        cleaned
    }
}

// --- serving ----------------------------------------------------------------

/// `GET /api/img/:id` — the byte-exact original, EXIF and all.
pub async fn serve_original(
    State(s): State<AppState>,
    Path(id): Path<String>,
) -> Result<Response, ApiError> {
    serve(s, &id, None).await
}

/// `GET /api/img/:id/:variant` — `thumb` (~400px) or `agent` (≤1568px).
pub async fn serve_variant(
    State(s): State<AppState>,
    Path((id, variant)): Path<(String, String)>,
) -> Result<Response, ApiError> {
    let variant = Variant::parse(&variant)?;
    serve(s, &id, Some(variant)).await
}

async fn serve(s: AppState, raw_id: &str, variant: Option<Variant>) -> Result<Response, ApiError> {
    let id = ImgId::parse(raw_id)?;
    let row = repo::get_attachment(&s.pool, id.node_id())
        .await?
        .ok_or_else(|| not_found(format!("no attachment {id}")))?;

    let mime = match variant {
        None => row.mime.clone(),
        Some(v) => row
            .variants
            .iter()
            .find(|stored| stored.variant == v.as_str())
            .map(|stored| stored.mime.clone())
            .ok_or_else(|| not_found(format!("attachment {id} has no {v} variant")))?,
    };
    let path = match variant {
        None => s.images.original_path(id, &mime)?,
        Some(v) => s.images.variant_path(id, v, &mime)?,
    };

    let store = s.images.clone();
    let bytes = tokio::task::spawn_blocking(move || store.read(&path)).await??;
    // A row with no bytes is a real state — a restore that missed the volume,
    // or a purge that ran ahead of its rows — and it gets its own message
    // rather than the "no such attachment" one, because the two need different
    // fixes and only this distinction says which.
    let bytes = bytes.ok_or_else(|| {
        ApiError(anyhow::anyhow!(
            "attachment {id} is recorded but its bytes are missing from the store"
        ))
    })?;

    let mime = HeaderValue::from_str(&mime)
        .unwrap_or_else(|_| HeaderValue::from_static("application/octet-stream"));
    Ok((
        StatusCode::OK,
        [
            (header::CONTENT_TYPE, mime),
            // An attachment's bytes never change: the id is the node id, and a
            // re-upload is a different node. Immutable caching is therefore a
            // statement of fact rather than a gamble, and it is what keeps a
            // work item with a dozen thumbnails from re-fetching them on every
            // render.
            (
                header::CACHE_CONTROL,
                HeaderValue::from_static("public, max-age=31536000, immutable"),
            ),
        ],
        bytes,
    )
        .into_response())
}

// --- lifecycle --------------------------------------------------------------

#[derive(Debug, Deserialize)]
pub struct LinkBody {
    /// The node claiming this image — a work item, a card, a proposal.
    pub owner_node_id: i64,
}

/// `POST /api/img/:id/link` — the save step of paste-before-save.
///
/// Idempotent: linking an image to an owner it already has returns the same
/// attachment rather than failing, because a retried save must not be an error.
pub async fn link_image(
    State(s): State<AppState>,
    Path(id): Path<String>,
    Json(b): Json<LinkBody>,
) -> ApiResult {
    let id = ImgId::parse(&id)?;
    let row = repo::attach_attachment(&s.pool, id.node_id(), b.owner_node_id).await?;
    Ok(Json(json!(row)))
}

/// `DELETE /api/img/:id` — the explicit discard: record and every blob.
///
/// A hard delete, not an archive. korg archives things that have a history
/// worth keeping; an image somebody discarded is bytes.
pub async fn delete_image(State(s): State<AppState>, Path(id): Path<String>) -> ApiResult {
    let id = ImgId::parse(&id)?;
    let deleted = repo::delete_attachment(&s.pool, id.node_id()).await?;
    if deleted {
        let store = s.images.clone();
        // Blobs after the row, as everywhere else here. A failure leaves
        // stranded bytes and is logged rather than failing the request: the
        // caller's attachment *is* gone, and reporting an error would invite a
        // retry that can only report the same thing.
        if let Err(e) = tokio::task::spawn_blocking(move || store.remove(id)).await? {
            tracing::error!(img_id = %id, error = %e, "attachment deleted but its blobs were not");
        }
    }
    Ok(Json(json!({ "deleted": deleted })))
}

/// `GET /api/img/stats` — what kmon's growth milestones read (slice 4).
///
/// `total_bytes` is what korg *believes* the store holds, summed from the
/// recorded sizes rather than measured off the disk. The gap between it and
/// `du` on the volume is the stranded-blob count, and having both numbers is
/// what makes that gap visible at all.
pub async fn stats(State(s): State<AppState>) -> ApiResult {
    let stats = repo::attachment_stats(&s.pool).await?;
    // Serialized from the struct rather than field-by-field, so `oldest_pending`
    // keeps the RFC 3339 encoding `AttachmentStats` declares. Rebuilding the
    // object by hand here spelled it `OffsetDateTime::to_string()` instead —
    // `2026-08-08 18:41:40.92767 +00:00:00`, which is every korg timestamp's
    // format except this one, for no reason a consumer could have guessed.
    let mut body = serde_json::to_value(&stats)?;
    // Two things kmon needs that are properties of the *store*, not of the rows.
    if let Some(map) = body.as_object_mut() {
        map.insert("root".into(), json!(s.images.root().display().to_string()));
        map.insert(
            "pending_ttl_hours".into(),
            json!(ATTACHMENT_PENDING_TTL_HOURS),
        );
    }
    Ok(Json(body))
}

/// `POST /api/img/sweep` — run the pending-orphan sweep now.
///
/// The same call the background task makes, exposed so it is testable, so a
/// runbook can force it, and so "did the sweeper run?" is answerable by running
/// it rather than by reading logs.
pub async fn sweep_now(State(s): State<AppState>) -> ApiResult {
    let swept = sweep(&s).await?;
    Ok(Json(json!({ "swept": swept.len(), "node_ids": swept })))
}

/// Delete every pending attachment past its TTL, and their blobs. Returns the
/// node ids taken.
pub async fn sweep(s: &AppState) -> anyhow::Result<Vec<i64>> {
    let swept = repo::sweep_pending_attachments(&s.pool, ATTACHMENT_PENDING_TTL_HOURS).await?;
    for node_id in &swept {
        let Ok(id) = ImgId::from_node_id(*node_id) else {
            continue;
        };
        let store = s.images.clone();
        if let Err(e) = tokio::task::spawn_blocking(move || store.remove(id)).await? {
            tracing::error!(img_id = %id, error = %e, "swept attachment's blobs survived");
        }
    }
    Ok(swept)
}

/// The background sweeper (handoff D5).
///
/// korg deliberately runs no scheduler for *work* — #581 and #950 settled that
/// an unattended job deciding something is due is a job that lies when it dies.
/// This is not that: it decides nothing, it deletes records that provably have
/// no owner, and it says so in the log every time it takes something. The
/// alternative — GC only when a human remembers — is how a store fills up.
pub fn spawn_sweeper(state: AppState) {
    tokio::spawn(async move {
        let mut ticker = tokio::time::interval(std::time::Duration::from_secs(
            SWEEP_INTERVAL_HOURS * 60 * 60,
        ));
        // The first tick fires immediately, which is wanted: a restart is a
        // good moment to collect what accumulated while korg was down.
        loop {
            ticker.tick().await;
            match sweep(&state).await {
                Ok(swept) if swept.is_empty() => {}
                Ok(swept) => tracing::info!(
                    count = swept.len(),
                    ttl_hours = ATTACHMENT_PENDING_TTL_HOURS,
                    "swept pending attachments"
                ),
                Err(e) => tracing::error!(error = %e, "attachment sweep failed"),
            }
        }
    });
}

// --- helpers ----------------------------------------------------------------

fn not_found(msg: String) -> ApiError {
    ApiError(korg_core::error::RepoError::NotFound(msg).into())
}

/// Where the store lives. Defaults to the path the Dockerfile sets, which the
/// compose file bind-mounts `/datastore/korg/images` onto (handoff D7) — so the
/// default and the deployed value agree, and a local run without the variable
/// still gets a coherent error rather than writing images somewhere surprising.
pub fn store_from_env() -> Store {
    Store::new(std::env::var("KORG_IMG_ROOT").unwrap_or_else(|_| "/data/images".to_string()))
}
