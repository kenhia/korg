//! Attachments (images: #582, #1119)
//!
//! An attachment is a node like any other (0027, the 0008/0010/0017 pattern):
//! a detail table for its metadata, and the image itself on disk under a
//! directory named for its display id. korg-img owns the bytes; this owns the
//! record of them.
//!
//! Two things about this kind are unlike the rest of korg and both are
//! deliberate (see 0027's header and the program handoff, korg:1128):
//!
//!   * **`state` is derived, never stored.** `pending` means "no has_attachment
//!     edge" and `linked` means "has one". The sweeper collects the former, so
//!     making the edge the single source of truth is what stops a generically
//!     `relate()`d image from being deleted out from under its owner.
//!   * **The bytes are not transactional with Postgres.** `create_attachment`
//!     commits the row and the caller then writes the blobs; a failed write is
//!     compensated by deleting the row, and if even that fails the sweeper
//!     collects it, because it has no edge. The reverse order would leave a
//!     committed row promising bytes that are not there — a broken image in
//!     every render rather than a temporary orphan nobody sees.

use anyhow::Result;
use serde::Serialize;
use sqlx::{PgPool, Row};
use time::OffsetDateTime;
use ts_rs::TS;

use crate::error::RepoError;

use super::common::{node_kind, require_kind, require_non_empty};
use super::relationships::relate;

/// The "no such attachment" message, naming **both** spellings of the id
/// (#1412, F-9 nit 5 of the 044–059 surface re-review).
///
/// An attachment has one identity in two notations: the decimal node id and
/// `img-<hex>`, which is that same number in hex. `get_attachment` accepts
/// either and resolves to the decimal one before it reads, so a miss on
/// `img-c2a` used to come back as `no attachment with node_id 3114` — accurate,
/// and momentarily unrecognisable as an answer to what was asked. Naming both
/// is the fix that does not need the caller's spelling threaded through every
/// layer that could produce the message, and it is strictly more useful than
/// echoing: the reader gets the translation whichever spelling they came in
/// with.
///
/// The hex half is omitted rather than faked for a non-positive id — node ids
/// are `BIGSERIAL` and `ImgId` refuses those, so there is no honest `img-`
/// spelling to print.
pub fn attachment_not_found(node_id: i64) -> String {
    match korg_img::ImgId::from_node_id(node_id) {
        Ok(img) => format!("no attachment with node_id {node_id} ({img})"),
        Err(_) => format!("no attachment with node_id {node_id}"),
    }
}

/// How long a `pending` attachment lives before the sweeper takes it — the
/// paste-before-save grace period (handoff D5). Long enough that an interrupted
/// edit survives a coffee break, short enough that a day's abandoned pastes do
/// not accumulate. **This is korg's entire garbage-collection story for
/// images**: there is no retention policy, no delete-on-close and no
/// delete-on-archive, by decision — growth is watched by kmon milestones
/// instead, and fast growth is the trigger to revisit.
pub const ATTACHMENT_PENDING_TTL_HOURS: i64 = 24;

/// One variant to record alongside a new attachment.
#[derive(Debug, Clone)]
pub struct NewAttachmentVariant {
    pub variant: String,
    pub mime: String,
    pub width: i32,
    pub height: i32,
    pub byte_size: i64,
}

/// `create_attachment` — the metadata half of an upload.
///
/// Not a `Deserialize`/`JsonSchema` type, unlike every other `New*` here: an
/// upload arrives as multipart bytes, and every field below is *measured* from
/// those bytes rather than asserted by the caller. A client cannot send a
/// width, and must not be able to send a mime type (see [`from_prepared`]).
///
/// [`from_prepared`]: NewAttachment::from_prepared
#[derive(Debug, Clone)]
pub struct NewAttachment {
    pub filename: String,
    pub mime: String,
    pub byte_size: i64,
    pub width: i32,
    pub height: i32,
    pub content_hash: String,
    pub variants: Vec<NewAttachmentVariant>,
    /// The node this image belongs to. `None` is the paste-before-save case:
    /// the attachment is created `pending` and linked when its owner is saved.
    pub owner_node_id: Option<i64>,
    pub tags: Vec<String>,
}

impl NewAttachment {
    /// Build from what korg-img measured, which is the only supported way to
    /// build one: `mime` and the dimensions come from **decoding the bytes**,
    /// never from the upload's declared `Content-Type`. A declared type is a
    /// claim by the client; this is what the image actually is.
    pub fn from_prepared(
        filename: impl Into<String>,
        prepared: &korg_img::Prepared,
        owner_node_id: Option<i64>,
    ) -> Self {
        Self {
            filename: filename.into(),
            mime: prepared.mime.to_string(),
            byte_size: prepared.byte_size as i64,
            width: prepared.width as i32,
            height: prepared.height as i32,
            content_hash: prepared.content_hash.clone(),
            variants: prepared
                .variants
                .iter()
                .map(|v| NewAttachmentVariant {
                    variant: v.variant.as_str().to_string(),
                    mime: v.mime.to_string(),
                    width: v.width as i32,
                    height: v.height as i32,
                    byte_size: v.bytes.len() as i64,
                })
                .collect(),
            owner_node_id,
            tags: Vec::new(),
        }
    }
}

/// One generated size, as a read surface returns it.
#[derive(Debug, Clone, Serialize, TS)]
#[ts(export, export_to = "korg.ts")]
pub struct AttachmentVariantRow {
    /// `thumb` or `agent` — korg-img's vocabulary.
    pub variant: String,
    pub mime: String,
    pub width: i32,
    pub height: i32,
    pub byte_size: i64,
    /// Where to fetch these bytes. Relative — see `korg_img::ROUTE_PREFIX`.
    pub url: String,
    /// True when these bytes **are** the original's (#1146): the original was
    /// already inside this size's ceiling and re-encoding it came out no
    /// smaller, so korg kept one copy instead of two.
    ///
    /// The `url` still works and still serves this size — that is the whole
    /// point — so a caller fetching images can ignore this field entirely. It
    /// is here for the ones that would otherwise be misled: an accounting
    /// reader must not add `byte_size` to the original's, and a caller that
    /// already holds the original can skip the fetch outright.
    pub is_original: bool,
}

/// An attachment as every read returns it.
#[derive(Debug, Clone, Serialize, TS)]
#[ts(export, export_to = "korg.ts")]
pub struct AttachmentRow {
    pub node_id: i64,
    /// The display id, `img-<hex of node_id>` (handoff D3) — what appears in a
    /// markdown token and what a serve route is addressed by.
    pub img_id: String,
    pub filename: String,
    pub mime: String,
    pub byte_size: i64,
    pub width: i32,
    pub height: i32,
    /// SHA-256 of the original bytes, recorded for future dedup *detection*.
    /// korg never shares a blob on it (handoff D4).
    pub content_hash: String,
    /// `pending` (no owner yet) or `linked`. **Derived from the edges on every
    /// read**, so it cannot disagree with them.
    pub state: String,
    /// The nodes that own this image, via `has_attachment`. Empty means
    /// `pending`, and means the sweeper will take it once it is old enough.
    pub owner_node_ids: Vec<i64>,
    /// Where to fetch the byte-exact original. Relative.
    pub url: String,
    pub variants: Vec<AttachmentVariantRow>,
    pub project: Option<String>,
    pub tags: Vec<String>,
    pub archived: bool,
    pub comment_count: i64,
    #[serde(with = "time::serde::rfc3339")]
    #[ts(type = "string")]
    pub created: OffsetDateTime,
    #[serde(with = "time::serde::rfc3339")]
    #[ts(type = "string")]
    pub updated: OffsetDateTime,
}

#[derive(sqlx::FromRow)]
struct AttachmentBase {
    node_id: i64,
    filename: String,
    mime: String,
    byte_size: i64,
    width: i32,
    height: i32,
    content_hash: String,
    owner_node_ids: Vec<i64>,
    project: Option<String>,
    tags: Vec<String>,
    archived: bool,
    comment_count: i64,
    created: OffsetDateTime,
    updated: OffsetDateTime,
}

const ATTACHMENT_SELECT: &str = "SELECT a.node_id, a.filename, a.mime, a.byte_size, \
            a.width, a.height, a.content_hash, \
            pj.name AS project, n.tags, n.archived, \
            (SELECT count(*) FROM comment cm WHERE cm.node_id = a.node_id) AS comment_count, \
            (SELECT coalesce(array_agg(r.left_id ORDER BY r.left_id), '{}') \
               FROM relationship r \
              WHERE r.right_id = a.node_id \
                AND r.relationship = 'has_attachment') AS owner_node_ids, \
            n.created, n.updated \
     FROM attachment a \
     JOIN node n ON n.id = a.node_id \
     LEFT JOIN project pj ON pj.id = n.project_id";

#[derive(sqlx::FromRow)]
struct VariantRow {
    node_id: i64,
    variant: String,
    mime: String,
    width: i32,
    height: i32,
    byte_size: i64,
}

/// The `variants` block of one attachment: every stored size, plus the ones
/// korg deliberately did not store.
///
/// #1146 lets `prepare` drop a variant whose re-encode came out no smaller than
/// an original that already fits — so since that change, "no `agent` row" is a
/// normal state meaning *the original is the agent variant*, not a missing one.
/// The read surface hides that: the size is still listed and its URL still
/// serves, because an agent being told to fetch `/agent` should never have to
/// care how korg stored it. Only `is_original` gives it away, for the callers
/// that need to know (byte accounting, and anyone already holding the original).
///
/// Reconstructed from the original's own dimensions rather than from a stored
/// flag, so it needs no migration and cannot fall out of step with the blobs:
/// [`korg_img::Variant::fits_original`] is the same predicate `prepare` used.
/// An absent row for a size the original does NOT fit stays absent — that one
/// is a genuinely missing variant, and it still 404s.
///
/// Order is korg-img's, smallest ceiling first, with any name korg-img no
/// longer knows tailing alphabetically. Stable whatever the storage did.
fn attachment_variants(
    all: &[VariantRow],
    base: &AttachmentBase,
    id: korg_img::ImgId,
) -> Vec<AttachmentVariantRow> {
    let stored: Vec<&VariantRow> = all.iter().filter(|v| v.node_id == base.node_id).collect();

    let mut rows: Vec<AttachmentVariantRow> = stored
        .iter()
        .map(|v| AttachmentVariantRow {
            variant: v.variant.clone(),
            mime: v.mime.clone(),
            width: v.width,
            height: v.height,
            byte_size: v.byte_size,
            url: match korg_img::Variant::parse(&v.variant) {
                Ok(known) => id.variant_url(known),
                // A variant name korg-img no longer knows: report the row
                // rather than dropping it, and let the URL 404 honestly
                // instead of pretending the size is gone.
                Err(_) => format!("{}/{}", id.url(), v.variant),
            },
            is_original: false,
        })
        .collect();

    for variant in korg_img::VARIANTS {
        let missing = !stored.iter().any(|v| v.variant == variant.as_str());
        if missing && variant.fits_original(base.width.max(0) as u32, base.height.max(0) as u32) {
            rows.push(AttachmentVariantRow {
                variant: variant.as_str().to_string(),
                mime: base.mime.clone(),
                width: base.width,
                height: base.height,
                // The original's size, because that is what fetching this URL
                // actually costs. `is_original` is what stops an accounting
                // reader adding it to the original's again — a zero here would
                // have balanced that sum by lying about the payload.
                byte_size: base.byte_size,
                url: id.variant_url(variant),
                is_original: true,
            });
        }
    }

    rows.sort_by_key(|r| match korg_img::Variant::parse(&r.variant) {
        Ok(known) => (
            korg_img::VARIANTS
                .iter()
                .position(|v| *v == known)
                .unwrap_or(usize::MAX),
            String::new(),
        ),
        Err(_) => (usize::MAX, r.variant.clone()),
    });
    rows
}

/// Turn base rows into full ones, fetching every variant in **one** query
/// rather than one per attachment — a work item with six screenshots would
/// otherwise be seven round-trips for a read that is meant to be free.
async fn hydrate_attachments(
    pool: &PgPool,
    bases: Vec<AttachmentBase>,
) -> Result<Vec<AttachmentRow>> {
    if bases.is_empty() {
        return Ok(Vec::new());
    }
    let ids: Vec<i64> = bases.iter().map(|b| b.node_id).collect();

    let variants = sqlx::query_as::<_, VariantRow>(
        "SELECT node_id, variant, mime, width, height, byte_size \
           FROM attachment_variant WHERE node_id = ANY($1) ORDER BY node_id, variant",
    )
    .bind(&ids)
    .fetch_all(pool)
    .await?;

    let mut rows = Vec::with_capacity(bases.len());
    for base in bases {
        // Non-positive node ids cannot exist (`BIGSERIAL`), so this is
        // unreachable in practice; refusing loudly beats emitting a row whose
        // `img_id` and `url` are silently wrong.
        let id = korg_img::ImgId::from_node_id(base.node_id)?;
        let state = if base.owner_node_ids.is_empty() {
            "pending"
        } else {
            "linked"
        };
        let variant_rows = attachment_variants(&variants, &base, id);
        rows.push(AttachmentRow {
            node_id: base.node_id,
            img_id: id.to_string(),
            filename: base.filename,
            mime: base.mime,
            byte_size: base.byte_size,
            width: base.width,
            height: base.height,
            content_hash: base.content_hash,
            state: state.to_string(),
            owner_node_ids: base.owner_node_ids,
            url: id.url(),
            variants: variant_rows,
            project: base.project,
            tags: base.tags,
            archived: base.archived,
            comment_count: base.comment_count,
            created: base.created,
            updated: base.updated,
        });
    }
    Ok(rows)
}

/// Record an uploaded image. The blobs are written by the caller **after** this
/// returns — see the module comment above for why that order, and
/// [`delete_attachment`] for the compensating action.
pub async fn create_attachment(pool: &PgPool, new: NewAttachment) -> Result<AttachmentRow> {
    require_non_empty(&new.filename, "attachment filename")?;
    if korg_img::ext_for_mime(&new.mime).is_none() {
        return Err(RepoError::invalid(format!(
            "korg does not store images of type '{}'",
            new.mime
        ))
        .into());
    }
    if new.byte_size <= 0 || new.width <= 0 || new.height <= 0 {
        return Err(RepoError::invalid(
            "an attachment must have a positive size and positive dimensions",
        )
        .into());
    }

    // The owner is resolved before the transaction so a typo'd node id is a
    // clean `not_found` naming it rather than the raw FK violation the edge
    // insert would surface as `internal` (WI #524).
    let mut project_id = None;
    if let Some(owner) = new.owner_node_id {
        let kind = node_kind(pool, owner).await?;
        if kind == "attachment" {
            return Err(RepoError::invalid(format!(
                "node {owner} is an attachment — an image cannot own another image"
            ))
            .into());
        }
        // Inherit the owner's project, so an image is filed the way the work it
        // belongs to is. An unfiled owner leaves it unfiled, which is honest.
        project_id =
            sqlx::query_scalar::<_, Option<i64>>("SELECT project_id FROM node WHERE id = $1")
                .bind(owner)
                .fetch_one(pool)
                .await?;
    }

    let mut tx = pool.begin().await?;
    let node_id: i64 = sqlx::query(
        "INSERT INTO node (kind, project_id, tags) VALUES ('attachment', $1, $2) RETURNING id",
    )
    .bind(project_id)
    .bind(&new.tags)
    .fetch_one(&mut *tx)
    .await?
    .get("id");

    sqlx::query(
        "INSERT INTO attachment \
             (node_id, filename, mime, byte_size, width, height, content_hash) \
         VALUES ($1, $2, $3, $4, $5, $6, $7)",
    )
    .bind(node_id)
    .bind(&new.filename)
    .bind(&new.mime)
    .bind(new.byte_size)
    .bind(new.width)
    .bind(new.height)
    .bind(&new.content_hash)
    .execute(&mut *tx)
    .await?;

    for variant in &new.variants {
        sqlx::query(
            "INSERT INTO attachment_variant \
                 (node_id, variant, mime, width, height, byte_size) \
             VALUES ($1, $2, $3, $4, $5, $6)",
        )
        .bind(node_id)
        .bind(&variant.variant)
        .bind(&variant.mime)
        .bind(variant.width)
        .bind(variant.height)
        .bind(variant.byte_size)
        .execute(&mut *tx)
        .await?;
    }

    // Owner -> attachment, matching the registry's orientation. Written here
    // rather than through `relate` for the same reason `create_handoff` does:
    // it is correct by construction inside the transaction that created both
    // ends, and it must not be able to half-commit.
    if let Some(owner) = new.owner_node_id {
        sqlx::query(
            "INSERT INTO relationship (left_id, right_id, relationship, created, origin) \
             VALUES ($1, $2, 'has_attachment', now(), 'create_attachment') \
             ON CONFLICT (left_id, right_id, relationship) \
             DO UPDATE SET left_id = relationship.left_id",
        )
        .bind(owner)
        .bind(node_id)
        .execute(&mut *tx)
        .await?;
    }

    tx.commit().await?;
    get_attachment(pool, node_id)
        .await?
        .ok_or_else(|| RepoError::NotFound(attachment_not_found(node_id)).into())
}

/// One attachment by node id. `None` when that node is missing or is not an
/// attachment (transports turn that into 404 / isError per D-6).
pub async fn get_attachment(pool: &PgPool, node_id: i64) -> Result<Option<AttachmentRow>> {
    let bases =
        sqlx::query_as::<_, AttachmentBase>(&format!("{ATTACHMENT_SELECT} WHERE a.node_id = $1"))
            .bind(node_id)
            .fetch_all(pool)
            .await?;
    Ok(hydrate_attachments(pool, bases).await?.pop())
}

/// Every image a node owns, oldest first — the attachment list under a work
/// item's body. Returns an empty vec for a node with none, and for a node that
/// does not exist: this is a projection of a node's edges, and `get_work_item`
/// is what says whether the node is there.
pub async fn list_attachments(pool: &PgPool, owner_node_id: i64) -> Result<Vec<AttachmentRow>> {
    let bases = sqlx::query_as::<_, AttachmentBase>(&format!(
        "{ATTACHMENT_SELECT} \
         JOIN relationship own ON own.right_id = a.node_id \
                              AND own.relationship = 'has_attachment' \
                              AND own.left_id = $1 \
         ORDER BY a.node_id"
    ))
    .bind(owner_node_id)
    .fetch_all(pool)
    .await?;
    hydrate_attachments(pool, bases).await
}

/// Attach a `pending` image to its owner — the save step of paste-before-save.
///
/// The edge is written through [`relate`], not around it, so the registry's
/// endpoint-kind rule is enforced on exactly one path. The attachment also
/// inherits the owner's project if it does not have one yet, which is what
/// makes a pasted-then-saved image filed the same way one uploaded straight
/// onto a work item is.
pub async fn attach_attachment(
    pool: &PgPool,
    node_id: i64,
    owner_node_id: i64,
) -> Result<AttachmentRow> {
    require_kind(pool, node_id, "attachment", "attachment").await?;
    relate(
        pool,
        owner_node_id,
        node_id,
        "has_attachment",
        Some("attach_attachment"),
        None,
    )
    .await?;
    sqlx::query(
        "UPDATE node SET project_id = (SELECT project_id FROM node WHERE id = $2) \
          WHERE id = $1 AND project_id IS NULL",
    )
    .bind(node_id)
    .bind(owner_node_id)
    .execute(pool)
    .await?;
    get_attachment(pool, node_id)
        .await?
        .ok_or_else(|| RepoError::NotFound(attachment_not_found(node_id)).into())
}

/// Delete an attachment's record. The caller removes its blobs.
///
/// This is the *discard* the UI's attachment list offers and the compensating
/// action for a failed blob write — deliberately a hard delete rather than an
/// archive, because an image nobody wants is bytes, not history. `false` means
/// there was no such attachment, which is not an error: cleaning up after a
/// write that never landed is the ordinary way to reach it.
pub async fn delete_attachment(pool: &PgPool, node_id: i64) -> Result<bool> {
    // Cascades to `attachment`, `attachment_variant`, the edges and any
    // comments, all declared ON DELETE CASCADE.
    let deleted = sqlx::query("DELETE FROM node WHERE id = $1 AND kind = 'attachment'")
        .bind(node_id)
        .execute(pool)
        .await?
        .rows_affected();
    Ok(deleted > 0)
}

/// Delete every `pending` attachment older than `ttl_hours`, returning the node
/// ids taken so the caller can remove their blobs.
///
/// **This is the whole of korg's image garbage collection** (handoff D5). The
/// predicate is *edge-less* rather than a stored state, which is what makes it
/// safe: an image reached by any `has_attachment` edge — however that edge was
/// written, including through the generic `relate` — is never a candidate.
///
/// Rows go first and blobs second. A crash between the two strands bytes on
/// disk, which costs space and is reconcilable; the other order would leave a
/// row promising bytes that no longer exist, which renders as a broken image
/// forever.
pub async fn sweep_pending_attachments(pool: &PgPool, ttl_hours: i64) -> Result<Vec<i64>> {
    let swept: Vec<i64> = sqlx::query_scalar(
        "DELETE FROM node n \
          WHERE n.kind = 'attachment' \
            AND n.created < now() - make_interval(hours => $1::int) \
            AND NOT EXISTS ( \
                  SELECT 1 FROM relationship r \
                   WHERE r.right_id = n.id AND r.relationship = 'has_attachment') \
        RETURNING n.id",
    )
    .bind(ttl_hours)
    .fetch_all(pool)
    .await?;
    Ok(swept)
}

/// What `/api/img/stats` reports — the numbers kmon's growth milestones watch
/// (slice 4). Byte totals come from the recorded sizes rather than from walking
/// the store, so this is two aggregates rather than a directory crawl.
#[derive(Debug, Clone, Serialize, TS)]
#[ts(export, export_to = "korg.ts")]
pub struct AttachmentStats {
    pub count: i64,
    /// Attachments with no owner yet — the sweeper's candidates.
    pub pending: i64,
    pub linked: i64,
    /// Bytes of the byte-exact originals.
    pub original_bytes: i64,
    /// Bytes of every generated variant.
    pub variant_bytes: i64,
    /// What the store should be holding, as korg believes it. Compare against
    /// `du` on the volume: a gap is stranded blobs (see
    /// [`sweep_pending_attachments`]) and is what slice 3's runbook reconciles.
    pub total_bytes: i64,
    /// When the oldest pending attachment was uploaded — how far behind the
    /// sweeper is, and null when nothing is pending.
    #[serde(with = "time::serde::rfc3339::option")]
    #[ts(type = "string | null")]
    pub oldest_pending: Option<OffsetDateTime>,
}

pub async fn attachment_stats(pool: &PgPool) -> Result<AttachmentStats> {
    const PENDING: &str = "NOT EXISTS (SELECT 1 FROM relationship r \
                                        WHERE r.right_id = a.node_id \
                                          AND r.relationship = 'has_attachment')";
    let row = sqlx::query(&format!(
        "SELECT count(*) AS count, \
                count(*) FILTER (WHERE {PENDING}) AS pending, \
                coalesce(sum(a.byte_size), 0)::bigint AS original_bytes, \
                min(n.created) FILTER (WHERE {PENDING}) AS oldest_pending \
           FROM attachment a JOIN node n ON n.id = a.node_id"
    ))
    .fetch_one(pool)
    .await?;
    let variant_bytes: i64 =
        sqlx::query_scalar("SELECT coalesce(sum(byte_size), 0)::bigint FROM attachment_variant")
            .fetch_one(pool)
            .await?;

    let count: i64 = row.get("count");
    let pending: i64 = row.get("pending");
    let original_bytes: i64 = row.get("original_bytes");
    Ok(AttachmentStats {
        count,
        pending,
        linked: count - pending,
        original_bytes,
        variant_bytes,
        total_bytes: original_bytes + variant_bytes,
        oldest_pending: row.get("oldest_pending"),
    })
}
