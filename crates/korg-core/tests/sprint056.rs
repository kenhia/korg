//! Sprint 056 (#582 + #1119, proposal korg:1081) — the attachment node.
//!
//! The bytes half of this feature lives in korg-img (unit-tested there) and
//! korg-api (`tests/images.rs`, which drives real uploads through the router).
//! What is tested here is the half that decides whether an image *survives*:
//! the lifecycle, and specifically the decision to derive `pending`/`linked`
//! from the `has_attachment` edge rather than store it in a column.
//!
//! That decision is worth a suite because the failure it prevents is silent and
//! permanent. With a stored state column, an edge written through the generic
//! `relate()` path — which is the documented way to attach an existing image,
//! and the path the UI's "link this on save" step could easily grow into —
//! would leave the column reading `pending`, and the sweeper would then delete
//! an image a work item points at. Nothing would report it. The tests below
//! hold the two properties that make that unrepresentable: state is a function
//! of the edges, and the sweeper's predicate is edge-less-ness rather than a
//! recorded state.

use korg_core::repo::{
    self, attach_attachment, attachment_stats, create_attachment, create_work_item,
    delete_attachment, get_attachment, get_node_preview, get_work_item_detail, list_attachments,
    relate, sweep_pending_attachments, unrelate, NewAttachment, ATTACHMENT_PENDING_TTL_HOURS,
};
use korg_test_support::{fresh_korg, new, test_project};
use sqlx::PgPool;

/// A plausible attachment. Only `owner_node_id` varies across these tests, so
/// it is the argument; everything else is what korg-img would have measured off
/// a 1200×800 screenshot.
fn shot(owner_node_id: Option<i64>) -> NewAttachment {
    NewAttachment {
        owner_node_id,
        ..new::attachment("screenshot.png")
    }
}

/// Backdate a node so a TTL-based sweep can reach it without the test sleeping.
async fn age_hours(pool: &PgPool, node_id: i64, hours: i64) {
    sqlx::query("UPDATE node SET created = now() - make_interval(hours => $2::int) WHERE id = $1")
        .bind(node_id)
        .bind(hours)
        .execute(pool)
        .await
        .expect("backdate node");
}

// --- identity ---------------------------------------------------------------

/// The display id is the node id in hex (handoff D3), and every URL a read
/// hands out is built from it.
///
/// Asserted against a *computed* expectation rather than a literal, because the
/// node id depends on how many nodes the harness made first — but the
/// relationship between the two is the contract, and a markdown token written
/// today has to resolve years from now.
#[tokio::test]
async fn an_attachment_is_addressed_by_its_node_id_in_hex() {
    let (_pg, pool) = fresh_korg().await;
    test_project(&pool).await;

    let a = create_attachment(&pool, shot(None)).await.expect("create");
    assert_eq!(a.img_id, format!("img-{:x}", a.node_id));
    assert_eq!(a.url, format!("/api/img/{}", a.img_id));

    let thumb = a
        .variants
        .iter()
        .find(|v| v.variant == "thumb")
        .expect("thumb recorded");
    assert_eq!(thumb.url, format!("/api/img/{}/thumb", a.img_id));
    assert_eq!(
        a.variants.len(),
        2,
        "both eager variants are recorded at upload — korg does no on-demand resizing"
    );
}

// --- the lifecycle ----------------------------------------------------------

/// `pending` and `linked` are a function of the edges, computed on every read.
///
/// The load-bearing part is the second half: the edge is written through the
/// **generic `relate`**, not through korg's own attach path, and the state
/// still moves. A stored column would not have — and the image would then have
/// been swept out from under its owner.
#[tokio::test]
async fn state_follows_the_edge_however_the_edge_was_written() {
    let (_pg, pool) = fresh_korg().await;
    test_project(&pool).await;
    let wi = create_work_item(&pool, new::work_item("a bug with a screenshot"))
        .await
        .expect("wi");

    let a = create_attachment(&pool, shot(None)).await.expect("create");
    assert_eq!(a.state, "pending", "an unclaimed upload starts pending");
    assert!(a.owner_node_ids.is_empty());

    let rel = relate(
        &pool,
        wi.node_id,
        a.node_id,
        "has_attachment",
        Some("a test pretending to be an agent"),
        None,
    )
    .await
    .expect("generic relate");

    let a = get_attachment(&pool, a.node_id)
        .await
        .expect("read")
        .expect("still there");
    assert_eq!(
        a.state, "linked",
        "an edge written through relate() must move the state — a stored column \
         would not have, and the sweeper would then take a linked image"
    );
    assert_eq!(a.owner_node_ids, vec![wi.node_id]);

    // And back again: detaching is how you let an image go.
    unrelate(&pool, rel).await.expect("unrelate");
    let a = get_attachment(&pool, a.node_id)
        .await
        .expect("read")
        .expect("still there");
    assert_eq!(a.state, "pending");
    assert!(a.owner_node_ids.is_empty());
}

/// Uploading straight onto an owner is one call, and the image inherits that
/// owner's project — so an image is filed the way the work it belongs to is.
#[tokio::test]
async fn an_upload_scoped_to_an_owner_is_linked_and_filed_immediately() {
    let (_pg, pool) = fresh_korg().await;
    test_project(&pool).await;
    let wi = create_work_item(
        &pool,
        korg_core::repo::NewWorkItem {
            project: Some(korg_test_support::TEST_PROJECT.into()),
            ..new::work_item("a bug")
        },
    )
    .await
    .expect("wi");

    let a = create_attachment(&pool, shot(Some(wi.node_id)))
        .await
        .expect("create");
    assert_eq!(a.state, "linked");
    assert_eq!(a.owner_node_ids, vec![wi.node_id]);
    assert_eq!(a.project.as_deref(), Some(korg_test_support::TEST_PROJECT));
}

/// The paste-before-save path: created unfiled, then claimed on save, picking
/// up the owner's project at that point.
#[tokio::test]
async fn a_pending_image_inherits_its_project_when_it_is_claimed() {
    let (_pg, pool) = fresh_korg().await;
    test_project(&pool).await;
    let wi = create_work_item(
        &pool,
        korg_core::repo::NewWorkItem {
            project: Some(korg_test_support::TEST_PROJECT.into()),
            ..new::work_item("a bug")
        },
    )
    .await
    .expect("wi");

    let a = create_attachment(&pool, shot(None)).await.expect("create");
    assert!(a.project.is_none(), "nothing to inherit from yet");

    let a = attach_attachment(&pool, a.node_id, wi.node_id)
        .await
        .expect("attach");
    assert_eq!(a.state, "linked");
    assert_eq!(a.project.as_deref(), Some(korg_test_support::TEST_PROJECT));

    // Saving twice must not be an error — a retried save is ordinary.
    let again = attach_attachment(&pool, a.node_id, wi.node_id)
        .await
        .expect("re-attach is idempotent");
    assert_eq!(again.owner_node_ids, vec![wi.node_id]);
}

/// The registry's endpoint rule reaches the generic path: an edge pointing the
/// wrong way would orphan the image while looking attached.
#[tokio::test]
async fn has_attachment_refuses_an_endpoint_that_is_not_an_attachment() {
    let (_pg, pool) = fresh_korg().await;
    test_project(&pool).await;
    let wi = create_work_item(&pool, new::work_item("a bug"))
        .await
        .expect("wi");
    let a = create_attachment(&pool, shot(None)).await.expect("create");

    let backwards = relate(&pool, a.node_id, wi.node_id, "has_attachment", None, None).await;
    let msg = backwards
        .expect_err("reversed edge must be refused")
        .to_string();
    assert!(
        msg.contains("attachment") && msg.contains("right"),
        "the refusal should name the endpoint rule it broke, got: {msg}"
    );

    // And an image cannot own an image, which would make the sweeper's
    // liveness question circular.
    let other = create_attachment(&pool, shot(None)).await.expect("create");
    let nested = create_attachment(&pool, shot(Some(other.node_id))).await;
    assert!(
        nested.is_err(),
        "an attachment must not be able to own another attachment"
    );
}

// --- garbage collection -----------------------------------------------------

/// The whole of korg's image GC (handoff D5): pending, past its TTL, gone.
/// Everything else stays — including a pending image that is merely young, and
/// a linked image of any age, because there is deliberately no retention
/// policy, no delete-on-close and no delete-on-archive.
#[tokio::test]
async fn the_sweeper_takes_old_orphans_and_nothing_else() {
    let (_pg, pool) = fresh_korg().await;
    test_project(&pool).await;
    let wi = create_work_item(&pool, new::work_item("a bug"))
        .await
        .expect("wi");

    let fresh_orphan = create_attachment(&pool, shot(None)).await.expect("create");
    let old_orphan = create_attachment(&pool, shot(None)).await.expect("create");
    let old_linked = create_attachment(&pool, shot(Some(wi.node_id)))
        .await
        .expect("create");

    age_hours(&pool, old_orphan.node_id, ATTACHMENT_PENDING_TTL_HOURS + 1).await;
    age_hours(&pool, old_linked.node_id, ATTACHMENT_PENDING_TTL_HOURS * 30).await;

    let swept = sweep_pending_attachments(&pool, ATTACHMENT_PENDING_TTL_HOURS)
        .await
        .expect("sweep");
    assert_eq!(
        swept,
        vec![old_orphan.node_id],
        "only the aged orphan is collected"
    );

    assert!(get_attachment(&pool, old_orphan.node_id)
        .await
        .unwrap()
        .is_none());
    assert!(
        get_attachment(&pool, fresh_orphan.node_id)
            .await
            .unwrap()
            .is_some(),
        "a pending image inside its grace period is somebody still typing"
    );
    assert!(
        get_attachment(&pool, old_linked.node_id)
            .await
            .unwrap()
            .is_some(),
        "korg has NO retention policy — a linked image is kept however old it is"
    );

    // Idempotent: a second pass finds nothing, so an hourly sweeper is cheap.
    assert!(
        sweep_pending_attachments(&pool, ATTACHMENT_PENDING_TTL_HOURS)
            .await
            .expect("second sweep")
            .is_empty()
    );
}

/// A closed, archived work item keeps its screenshots.
///
/// This is the decision most likely to be "tidied up" later by someone
/// reasonably assuming that finished work does not need its images — so it gets
/// a test that says the quiet part: Ken's storage arithmetic (≈30K full-res
/// captures in 20 GB) says the space is not worth the risk of deleting evidence
/// somebody wanted, and kmon milestones watch growth instead.
#[tokio::test]
async fn closing_and_archiving_a_work_item_never_touches_its_images() {
    let (_pg, pool) = fresh_korg().await;
    test_project(&pool).await;
    let wi = create_work_item(&pool, new::work_item("a bug"))
        .await
        .expect("wi");
    let a = create_attachment(&pool, shot(Some(wi.node_id)))
        .await
        .expect("create");
    age_hours(&pool, a.node_id, ATTACHMENT_PENDING_TTL_HOURS * 365).await;

    repo::update_work_item(
        &pool,
        wi.wi_number,
        korg_core::repo::WorkItemPatch {
            wi_status: Some("closed".into()),
            archived: Some(true),
            ..Default::default()
        },
    )
    .await
    .expect("close and archive");

    let swept = sweep_pending_attachments(&pool, ATTACHMENT_PENDING_TTL_HOURS)
        .await
        .expect("sweep");
    assert!(swept.is_empty(), "no retention policy, by decision");
    assert_eq!(
        list_attachments(&pool, wi.node_id).await.unwrap().len(),
        1,
        "the image is still attached to the archived item"
    );
}

/// Deleting the *owner* orphans its images, and the sweeper then collects them
/// — which is correct GC, and the one path that reaches it. Worth pinning
/// because it is a consequence of deriving state from the edge rather than
/// something anybody wrote deliberately.
#[tokio::test]
async fn deleting_an_owner_leaves_its_images_for_the_sweeper() {
    let (_pg, pool) = fresh_korg().await;
    test_project(&pool).await;
    let card = repo::create_card(&pool, new::card("a card"))
        .await
        .expect("card");
    let a = create_attachment(&pool, shot(Some(card.node_id)))
        .await
        .expect("create");
    age_hours(&pool, a.node_id, ATTACHMENT_PENDING_TTL_HOURS + 1).await;

    sqlx::query("DELETE FROM node WHERE id = $1")
        .bind(card.node_id)
        .execute(&pool)
        .await
        .expect("delete the owner");

    let a = get_attachment(&pool, a.node_id)
        .await
        .unwrap()
        .expect("the image outlives its owner until the sweep");
    assert_eq!(
        a.state, "pending",
        "the cascade took the edge with the owner"
    );

    let swept = sweep_pending_attachments(&pool, ATTACHMENT_PENDING_TTL_HOURS)
        .await
        .expect("sweep");
    assert_eq!(swept, vec![a.node_id]);
}

/// Discard is a hard delete, and it takes the variant rows with it.
#[tokio::test]
async fn discarding_an_attachment_removes_every_record_of_it() {
    let (_pg, pool) = fresh_korg().await;
    test_project(&pool).await;
    let a = create_attachment(&pool, shot(None)).await.expect("create");

    assert!(delete_attachment(&pool, a.node_id).await.expect("delete"));
    assert!(get_attachment(&pool, a.node_id).await.unwrap().is_none());
    let orphaned_variants: i64 =
        sqlx::query_scalar("SELECT count(*) FROM attachment_variant WHERE node_id = $1")
            .bind(a.node_id)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(orphaned_variants, 0, "the variant rows cascade");

    assert!(
        !delete_attachment(&pool, a.node_id).await.expect("delete"),
        "a second discard is `false`, not an error — it is how a failed blob \
         write is cleaned up"
    );
}

// --- reads ------------------------------------------------------------------

/// `get_work_item` inlines the images (#1119) with everything an agent needs to
/// decide whether to fetch one, and does not *also* report them as bare edges.
#[tokio::test]
async fn a_work_item_read_carries_its_images_once() {
    let (_pg, pool) = fresh_korg().await;
    test_project(&pool).await;
    let wi = create_work_item(&pool, new::work_item("a bug with a screenshot"))
        .await
        .expect("wi");
    let a = create_attachment(&pool, shot(Some(wi.node_id)))
        .await
        .expect("create");

    let detail = get_work_item_detail(&pool, wi.wi_number)
        .await
        .expect("read")
        .expect("found");

    assert_eq!(detail.attachments.len(), 1);
    let inlined = &detail.attachments[0];
    assert_eq!(inlined.img_id, a.img_id);
    assert_eq!((inlined.width, inlined.height), (1200, 800));
    assert!(
        inlined.variants.iter().any(|v| v.variant == "agent"),
        "the agent variant's url is the whole point of inlining these"
    );

    assert!(
        !detail.related.iter().any(|r| r.label == "has_attachment"),
        "`attachments` carries this edge in full — reporting it in `related` too \
         would spend a slot of the cap saying the same thing twice"
    );
}

/// A `has_attachment` edge on a node that is *not* a work item still reads
/// usefully: `list_attachments` works for any kind, and the generic `related`
/// block resolves the neighbour to its filename rather than `attachment #3114`.
#[tokio::test]
async fn any_node_can_own_images_and_they_resolve_by_name() {
    let (_pg, pool) = fresh_korg().await;
    test_project(&pool).await;
    let proposal = repo::create_proposal(&pool, new::proposal("a proposal"))
        .await
        .expect("proposal");
    let a = create_attachment(&pool, shot(Some(proposal.row.node_id)))
        .await
        .expect("create");

    let listed = list_attachments(&pool, proposal.row.node_id)
        .await
        .expect("list");
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].node_id, a.node_id);

    let (related, _) = repo::related_context(&pool, proposal.row.node_id, None)
        .await
        .expect("related");
    let edge = related
        .iter()
        .find(|r| r.label == "has_attachment")
        .expect("the edge is inlined on a proposal");
    assert_eq!(
        edge.title, "screenshot.png",
        "an attachment resolves to its filename, not to `attachment #<id>`"
    );

    // A node with no images, and a node that does not exist, are both empty —
    // this read is a projection of edges, not an existence check.
    assert!(list_attachments(&pool, proposal.row.node_id + 9_999)
        .await
        .expect("list")
        .is_empty());
}

/// The generic preview renders an attachment's shape, not just its id — the
/// chain sprint 054 built (`NODE_KINDS` → the check constraint → a real preview
/// arm) applied to the ninth kind.
#[tokio::test]
async fn an_attachment_previews_its_shape() {
    let (_pg, pool) = fresh_korg().await;
    test_project(&pool).await;
    let a = create_attachment(&pool, shot(None)).await.expect("create");

    let preview = get_node_preview(&pool, a.node_id)
        .await
        .expect("preview")
        .expect("found");
    assert_eq!(preview.kind, "attachment");
    assert_eq!(preview.title, "screenshot.png");
    assert!(
        preview.badges.contains(&a.img_id) && preview.badges.contains(&"pending".to_string()),
        "the badges say which image this is and whether anything owns it, got {:?}",
        preview.badges
    );
    let field = |label: &str| {
        preview
            .fields
            .iter()
            .find(|f| f.label == label)
            .map(|f| f.value.clone())
    };
    assert_eq!(field("Dimensions").as_deref(), Some("1200×800"));
    assert_eq!(field("Type").as_deref(), Some("image/png"));
    assert_eq!(field("Original").as_deref(), Some(a.url.as_str()));
    assert!(
        field("agent").is_some(),
        "the variant urls are what make this panel useful before slice 2's lightbox"
    );
}

// --- stats ------------------------------------------------------------------

/// What kmon's growth milestones read (slice 4). `total_bytes` is korg's own
/// belief about the store, summed from recorded sizes — the number a `du` on
/// the volume is compared against to find stranded blobs.
#[tokio::test]
async fn stats_report_the_split_and_the_bytes() {
    let (_pg, pool) = fresh_korg().await;
    test_project(&pool).await;
    let wi = create_work_item(&pool, new::work_item("a bug"))
        .await
        .expect("wi");

    let empty = attachment_stats(&pool).await.expect("stats");
    assert_eq!((empty.count, empty.total_bytes), (0, 0));
    assert!(empty.oldest_pending.is_none());

    create_attachment(&pool, shot(Some(wi.node_id)))
        .await
        .expect("linked");
    let pending = create_attachment(&pool, shot(None)).await.expect("pending");
    age_hours(&pool, pending.node_id, 5).await;

    let stats = attachment_stats(&pool).await.expect("stats");
    assert_eq!((stats.count, stats.linked, stats.pending), (2, 1, 1));
    assert_eq!(stats.original_bytes, 80_000, "two 40 KB originals");
    assert_eq!(stats.variant_bytes, 66_000, "two sets of thumb + agent");
    assert_eq!(stats.total_bytes, 146_000);
    assert!(
        stats.oldest_pending.is_some(),
        "how far behind the sweeper is — null only when nothing is pending"
    );
}

// --- refusals ---------------------------------------------------------------

/// korg records what it can serve. A mime it has no extension for could be
/// written but never read back, so it is refused at the write boundary rather
/// than becoming an unreadable blob.
#[tokio::test]
async fn an_unservable_image_type_is_refused_at_the_write_boundary() {
    let (_pg, pool) = fresh_korg().await;
    test_project(&pool).await;

    let bad_mime = create_attachment(
        &pool,
        NewAttachment {
            mime: "image/tiff".into(),
            ..shot(None)
        },
    )
    .await;
    assert!(bad_mime.is_err(), "a type korg cannot serve is not stored");

    for broken in [
        NewAttachment {
            filename: "   ".into(),
            ..shot(None)
        },
        NewAttachment {
            byte_size: 0,
            ..shot(None)
        },
        NewAttachment {
            width: 0,
            ..shot(None)
        },
    ] {
        assert!(
            create_attachment(&pool, broken).await.is_err(),
            "a degenerate attachment must not reach the table"
        );
    }

    let missing_owner = create_attachment(&pool, shot(Some(999_999))).await;
    let msg = missing_owner
        .expect_err("an unknown owner is refused")
        .to_string();
    assert!(
        msg.contains("999999"),
        "the refusal should name the id that did not resolve, got: {msg}"
    );
}
