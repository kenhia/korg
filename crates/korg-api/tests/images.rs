//! Sprint 056 (#582 + #1119) — images over REST, end to end.
//!
//! The core suite (`korg-core/tests/sprint056.rs`) covers the lifecycle with
//! metadata handed straight to the repo. This one is about the half that only
//! exists once real bytes are involved: a genuine PNG going in through
//! multipart, being decoded, resized, written to disk, and coming back out of a
//! serve route with the right content type and the right *pixels*.
//!
//! It asserts against the store directory as well as against korg's responses,
//! because "korg says it stored an image" and "there is an image on disk" are
//! different claims and only the second one survives a restart.

mod common;
use common::{app_with_images, multipart, raw, req};

use axum::http::StatusCode;
use serde_json::{json, Value};

/// A recognisable test image: a gradient, so a resized copy is still visibly
/// the same picture and a mixed-up variant would not decode to these pixels.
fn png(width: u32, height: u32) -> Vec<u8> {
    let mut buf = image::RgbaImage::new(width, height);
    for (x, y, px) in buf.enumerate_pixels_mut() {
        *px = image::Rgba([(x % 256) as u8, (y % 256) as u8, 200, 255]);
    }
    let mut out = Vec::new();
    image::DynamicImage::ImageRgba8(buf)
        .write_to(&mut std::io::Cursor::new(&mut out), image::ImageFormat::Png)
        .expect("encode test png");
    out
}

async fn upload(
    router: &axum::Router,
    filename: &str,
    bytes: &[u8],
    owner: Option<i64>,
) -> (StatusCode, Value) {
    let (content_type, body) = multipart("file", filename, bytes);
    let path = match owner {
        Some(id) => format!("/api/img?owner={id}"),
        None => "/api/img".to_string(),
    };
    let (status, _, raw_body) = raw(router, "POST", &path, Some(&content_type), body).await;
    let json = serde_json::from_slice(&raw_body).unwrap_or(Value::Null);
    (status, json)
}

/// The round trip Ken's completion criteria name for an agent: upload with one
/// multipart POST, then fetch the `agent` variant and read it.
#[tokio::test]
async fn an_agent_uploads_an_image_and_reads_the_agent_variant_back() {
    let (_guard, _pool, router, root) = app_with_images().await;

    let original = png(2400, 1200);
    let (status, body) = upload(&router, "screenshot.png", &original, None).await;
    assert_eq!(status, StatusCode::OK, "{body}");

    let img_id = body["img_id"].as_str().expect("img_id").to_string();
    assert!(img_id.starts_with("img-"), "{img_id}");
    assert_eq!(
        body["mime"], "image/png",
        "the type is sniffed from the bytes"
    );
    assert_eq!(body["width"], 2400);
    assert_eq!(body["height"], 1200);
    assert_eq!(body["state"], "pending", "nothing owns it yet");
    assert_eq!(body["filename"], "screenshot.png");

    // The original comes back byte-for-byte: it is the archival copy.
    let (status, headers, served) = raw(
        &router,
        "GET",
        &format!("/api/img/{img_id}"),
        None,
        Vec::new(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(headers["content-type"], "image/png");
    assert_eq!(served, original, "the original is kept byte-exact");
    assert!(
        headers["cache-control"]
            .to_str()
            .unwrap()
            .contains("immutable"),
        "an attachment's bytes never change — the id is the node id"
    );

    // The agent variant is a real image, capped at the vision ceiling.
    let (status, headers, agent) = raw(
        &router,
        "GET",
        &format!("/api/img/{img_id}/agent"),
        None,
        Vec::new(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(headers["content-type"], "image/png");
    let decoded = image::load_from_memory(&agent).expect("the agent variant decodes");
    assert_eq!(
        decoded.width(),
        korg_img::AGENT_LONG_EDGE,
        "≤1568px on the long edge — bigger is bytes the model never sees"
    );
    assert_eq!(decoded.height(), korg_img::AGENT_LONG_EDGE / 2);

    let (status, _, thumb) = raw(
        &router,
        "GET",
        &format!("/api/img/{img_id}/thumb"),
        None,
        Vec::new(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        image::load_from_memory(&thumb)
            .expect("thumb decodes")
            .width(),
        korg_img::THUMB_LONG_EDGE
    );

    // And it is genuinely on disk, in one directory per attachment.
    let dir = korg_img::Store::new(&root).dir(korg_img::ImgId::parse(&img_id).unwrap());
    let mut names: Vec<String> = std::fs::read_dir(&dir)
        .expect("the attachment's directory exists")
        .flatten()
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    assert_eq!(names, ["agent.png", "original.png", "thumb.png"]);
}

/// Uploading onto a work item links it in one call, and `get_work_item` then
/// carries it — the #1119 read path, over the transport an agent uses.
#[tokio::test]
async fn an_image_uploaded_onto_a_work_item_shows_up_in_its_read() {
    let (_guard, _pool, router, _root) = app_with_images().await;

    let (status, wi) = req(
        &router,
        "POST",
        "/api/work-items",
        Some(json!({"title": "a bug", "content": "", "project": common::PROJECT})),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{wi}");
    let node_id = wi["node_id"].as_i64().expect("node_id");

    let (status, uploaded) = upload(&router, "bug.png", &png(800, 600), Some(node_id)).await;
    assert_eq!(status, StatusCode::OK, "{uploaded}");
    assert_eq!(uploaded["state"], "linked");
    assert_eq!(uploaded["project"], common::PROJECT);

    let (status, detail) = req(
        &router,
        "GET",
        &format!("/api/work-items/{}", wi["wi_number"].as_i64().unwrap()),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let attachments = detail["attachments"].as_array().expect("attachments block");
    assert_eq!(attachments.len(), 1);
    assert_eq!(attachments[0]["img_id"], uploaded["img_id"]);

    // Every url the read hands out actually resolves.
    for url in [attachments[0]["url"].as_str().unwrap()].into_iter().chain(
        attachments[0]["variants"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v["url"].as_str().unwrap()),
    ) {
        let (status, _, bytes) = raw(&router, "GET", url, None, Vec::new()).await;
        assert_eq!(status, StatusCode::OK, "{url} should serve");
        assert!(!bytes.is_empty(), "{url} served nothing");
    }
}

/// Paste-before-save: upload unowned, then claim it when the item is saved.
#[tokio::test]
async fn a_pending_image_is_claimed_by_its_owner_on_save() {
    let (_guard, _pool, router, _root) = app_with_images().await;

    let (_, pending) = upload(&router, "pasted.png", &png(300, 300), None).await;
    let img_id = pending["img_id"].as_str().unwrap().to_string();
    assert_eq!(pending["state"], "pending");

    let (_, wi) = req(
        &router,
        "POST",
        "/api/work-items",
        Some(
            json!({"title": "written after the paste", "content": "", "project": common::PROJECT}),
        ),
    )
    .await;
    let node_id = wi["node_id"].as_i64().unwrap();

    let (status, linked) = req(
        &router,
        "POST",
        &format!("/api/img/{img_id}/link"),
        Some(json!({"owner_node_id": node_id})),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{linked}");
    assert_eq!(linked["state"], "linked");
    assert_eq!(linked["owner_node_ids"], json!([node_id]));

    // A retried save is not an error.
    let (status, _) = req(
        &router,
        "POST",
        &format!("/api/img/{img_id}/link"),
        Some(json!({"owner_node_id": node_id})),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
}

/// Discard takes the record and the blobs together — the reason there is no
/// MCP delete tool: this is the one path that owns both.
#[tokio::test]
async fn discarding_an_image_removes_its_bytes_from_disk() {
    let (_guard, _pool, router, root) = app_with_images().await;

    let (_, uploaded) = upload(&router, "doomed.png", &png(200, 200), None).await;
    let img_id = uploaded["img_id"].as_str().unwrap().to_string();
    let dir = korg_img::Store::new(&root).dir(korg_img::ImgId::parse(&img_id).unwrap());
    assert!(dir.exists());

    let (status, body) = req(&router, "DELETE", &format!("/api/img/{img_id}"), None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["deleted"], true);
    assert!(!dir.exists(), "the blobs go with the record");

    let (status, _, _) = raw(
        &router,
        "GET",
        &format!("/api/img/{img_id}"),
        None,
        Vec::new(),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    let (status, body) = req(&router, "DELETE", &format!("/api/img/{img_id}"), None).await;
    assert_eq!(status, StatusCode::OK, "a second discard is not an error");
    assert_eq!(body["deleted"], false);
}

/// `/api/img/stats` is what kmon's milestones read, and `/api/img/sweep` is the
/// GC the background task runs on a timer.
#[tokio::test]
async fn stats_and_the_sweep_are_reachable_over_rest() {
    let (_guard, pool, router, root) = app_with_images().await;

    let (_, orphan) = upload(&router, "orphan.png", &png(500, 500), None).await;
    let img_id = orphan["img_id"].as_str().unwrap().to_string();
    let node_id = orphan["node_id"].as_i64().unwrap();

    let (status, stats) = req(&router, "GET", "/api/img/stats", None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(stats["count"], 1);
    assert_eq!(stats["pending"], 1);
    assert_eq!(stats["linked"], 0);
    assert!(stats["total_bytes"].as_i64().unwrap() > 0);
    assert!(stats["oldest_pending"].is_string());
    assert_eq!(stats["root"], root.display().to_string());

    // Nothing is old enough yet — the grace period is the whole point.
    let (status, swept) = req(&router, "POST", "/api/img/sweep", None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(swept["swept"], 0);

    sqlx::query("UPDATE node SET created = now() - interval '48 hours' WHERE id = $1")
        .bind(node_id)
        .execute(&pool)
        .await
        .expect("backdate");

    let (_, swept) = req(&router, "POST", "/api/img/sweep", None).await;
    assert_eq!(swept["swept"], 1);
    assert_eq!(swept["node_ids"], json!([node_id]));

    let dir = korg_img::Store::new(&root).dir(korg_img::ImgId::parse(&img_id).unwrap());
    assert!(!dir.exists(), "the sweeper removes blobs, not only rows");

    let (_, stats) = req(&router, "GET", "/api/img/stats", None).await;
    assert_eq!(stats["count"], 0);
    assert!(stats["oldest_pending"].is_null());
}

/// A static route segment must not be shootable by a plausible display id, and
/// the addressing must survive the spellings that actually turn up.
#[tokio::test]
async fn addressing_is_unambiguous() {
    let (_guard, _pool, router, _root) = app_with_images().await;
    let (_, uploaded) = upload(&router, "shot.png", &png(100, 100), None).await;
    let img_id = uploaded["img_id"].as_str().unwrap().to_string();

    // `stats` resolves as the stats route, not as an attachment called "stats".
    let (status, body) = req(&router, "GET", "/api/img/stats", None).await;
    assert_eq!(status, StatusCode::OK);
    assert!(
        body["count"].is_number(),
        "got an attachment instead: {body}"
    );

    // The brainstorm wrote ids uppercase; both spellings reach the same image.
    let (status, _, upper) = raw(
        &router,
        "GET",
        &format!("/api/img/{}", img_id.to_uppercase()),
        None,
        Vec::new(),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "IMG-C2A must resolve like img-c2a");
    assert!(!upper.is_empty());
}

/// Every refusal an upload can produce is the *caller's* problem, and must read
/// as one. Before #524 this class arrived as a 500 and korg's web client
/// rendered it as an apology with a retry suggestion — precisely the wrong
/// advice for a PDF renamed `.png`.
#[tokio::test]
async fn bad_uploads_and_bad_ids_are_400s_and_404s_never_500s() {
    let (_guard, _pool, router, _root) = app_with_images().await;

    let (status, body) = upload(&router, "notes.pdf", b"%PDF-1.7 not an image", None).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert_eq!(body["code"], "invalid_input");
    assert!(
        body["error"].as_str().unwrap().contains("PNG"),
        "the refusal should name what korg does accept, got: {body}"
    );

    // A multipart body with no file part at all.
    let (status, _, raw_body) = raw(
        &router,
        "POST",
        "/api/img",
        Some("multipart/form-data; boundary=korgtestboundary"),
        b"--korgtestboundary--\r\n".to_vec(),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    let body: Value = serde_json::from_slice(&raw_body).unwrap();
    assert_eq!(body["code"], "invalid_input");

    // A malformed id is the caller's typo; a well-formed one for an image that
    // does not exist is a 404. The distinction is what tells a client whether
    // to fix the request or to stop asking.
    let (status, _, raw_body) = raw(&router, "GET", "/api/img/not-an-id", None, Vec::new()).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    let body: Value = serde_json::from_slice(&raw_body).unwrap();
    assert_eq!(body["code"], "invalid_input");

    let (status, _, _) = raw(&router, "GET", "/api/img/img-ffffff", None, Vec::new()).await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    // An unknown variant name, on an image that does exist.
    let (_, uploaded) = upload(&router, "shot.png", &png(80, 80), None).await;
    let img_id = uploaded["img_id"].as_str().unwrap();
    let (status, _, raw_body) = raw(
        &router,
        "GET",
        &format!("/api/img/{img_id}/enormous"),
        None,
        Vec::new(),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    let body: Value = serde_json::from_slice(&raw_body).unwrap();
    assert!(
        body["error"].as_str().unwrap().contains("thumb"),
        "the refusal should name the variants that exist, got: {body}"
    );

    // Linking to a node that is not there.
    let (status, body) = req(
        &router,
        "POST",
        &format!("/api/img/{img_id}/link"),
        Some(json!({"owner_node_id": 999_999})),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{body}");
}

/// An upload past the cap is refused by the route's body limit rather than
/// buffered — axum's default is 2 MB, which would have refused most real
/// screenshots, so this pins that the limit was raised *and* still exists.
#[tokio::test]
async fn the_upload_cap_is_enforced_by_the_route() {
    let (_guard, _pool, router, _root) = app_with_images().await;

    let oversized = vec![0u8; korg_img::MAX_UPLOAD_BYTES + 1024];
    let (content_type, body) = multipart("file", "huge.png", &oversized);
    let (status, _, _) = raw(&router, "POST", "/api/img", Some(&content_type), body).await;
    assert_ne!(
        status,
        StatusCode::OK,
        "an oversized upload must be refused"
    );

    // And a comfortably-sized real screenshot is not caught by it. The fixture
    // is noise rather than the gradient the other tests use: PNG compresses a
    // gradient down to a few hundred KB however large the canvas, so a
    // gradient-based "big image" would sail under axum's 2 MB default and this
    // test would pass without ever exercising the raised limit.
    let big = noisy_png(1600, 900);
    assert!(
        big.len() > 2 * 1024 * 1024,
        "this fixture only tests the raised limit if it exceeds axum's 2 MB default \
         (got {} bytes)",
        big.len()
    );
    let (status, body) = upload(&router, "screenshot.png", &big, None).await;
    assert_eq!(status, StatusCode::OK, "{body}");
}

/// An incompressible PNG, for the one test that needs real size rather than
/// real dimensions. Deterministic (a plain LCG) so the byte count is stable.
fn noisy_png(width: u32, height: u32) -> Vec<u8> {
    let mut state: u32 = 0x1357_9bdf;
    let mut buf = image::RgbaImage::new(width, height);
    for px in buf.pixels_mut() {
        let mut next = || {
            state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            (state >> 24) as u8
        };
        *px = image::Rgba([next(), next(), next(), 255]);
    }
    let mut out = Vec::new();
    image::DynamicImage::ImageRgba8(buf)
        .write_to(&mut std::io::Cursor::new(&mut out), image::ImageFormat::Png)
        .expect("encode noisy png");
    out
}

// === #1146: the agent variant that is the original ==========================

/// A small paste stores ONE copy of itself and still answers `/agent`.
///
/// The bug this fences, from the images program close-out: `img-46b` on #1115
/// had a 43,911-byte `:agent` variant beside a 24,598-byte original at
/// identical 1019x311 dimensions. The re-encode was faithful and pointless —
/// the original was already inside the vision ceiling, and `image`'s PNG
/// encoder is worse at it than whatever produced the paste. Every screenshot
/// small enough to matter was paying double storage for a second copy.
///
/// What must hold afterwards is the whole contract in one place: the byte
/// saving is real (no second blob on disk), and NOTHING an agent does changes
/// — `/agent` still serves, still decodes, still declares its type. An agent
/// told to fetch the agent variant must never need to know how korg stored it.
#[tokio::test]
async fn a_small_paste_serves_its_agent_variant_from_the_original_blob() {
    let (_guard, _pool, router, root) = app_with_images().await;

    let original = png(1019, 311);
    let (status, body) = upload(&router, "paste.png", &original, None).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let img_id = body["img_id"].as_str().expect("img_id").to_string();

    // On disk: the original and a thumb, and no third file duplicating it.
    let dir = korg_img::Store::new(&root).dir(korg_img::ImgId::parse(&img_id).unwrap());
    let mut names: Vec<String> = std::fs::read_dir(&dir)
        .expect("the attachment's directory exists")
        .flatten()
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    assert_eq!(
        names,
        ["original.png", "thumb.png"],
        "no agent.png sitting beside an original it is a bigger copy of"
    );

    // In the read surface: both sizes are still listed, and the agent one says
    // where its bytes really come from.
    let variants = body["variants"].as_array().expect("variants block");
    assert_eq!(variants.len(), 2, "both sizes are still advertised: {body}");
    let agent_row = variants
        .iter()
        .find(|v| v["variant"] == "agent")
        .expect("the agent size is still listed");
    assert_eq!(agent_row["is_original"], true);
    assert_eq!(
        agent_row["byte_size"].as_u64().unwrap(),
        original.len() as u64,
        "it reports what fetching it actually costs, not zero"
    );
    assert_eq!(
        variants.iter().find(|v| v["variant"] == "thumb").unwrap()["is_original"],
        false,
        "the thumb is a real downscale and has its own blob"
    );

    // And over the wire the agent notices nothing at all.
    let (status, headers, served) = raw(
        &router,
        "GET",
        &format!("/api/img/{img_id}/agent"),
        None,
        Vec::new(),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "the agent URL still serves");
    assert_eq!(headers["content-type"], "image/png");
    assert_eq!(served, original, "and the bytes are the original's");
    let decoded = image::load_from_memory(&served).expect("it decodes");
    assert_eq!((decoded.width(), decoded.height()), (1019, 311));
}

/// The `agent` URL of an image too big to fit is unaffected — it has a real
/// downscaled blob, and #1146 must not have taught the serve path to fall back
/// to an oversized original when a variant is genuinely missing.
#[tokio::test]
async fn an_oversized_image_still_serves_a_downscaled_agent_variant() {
    let (_guard, _pool, router, _root) = app_with_images().await;

    let (_, body) = upload(&router, "wide.png", &png(3000, 500), None).await;
    let img_id = body["img_id"].as_str().unwrap();

    let agent_row = body["variants"]
        .as_array()
        .unwrap()
        .iter()
        .find(|v| v["variant"] == "agent")
        .expect("agent variant");
    assert_eq!(agent_row["is_original"], false);

    let (status, _, served) = raw(
        &router,
        "GET",
        &format!("/api/img/{img_id}/agent"),
        None,
        Vec::new(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        image::load_from_memory(&served).unwrap().width(),
        korg_img::AGENT_LONG_EDGE,
        "still capped at the vision ceiling"
    );
}
