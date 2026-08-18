//! korg-img — the image and blob engine behind korg's attachments (#582, #1119).
//!
//! Sprint 056, proposal `korg:1081`, slice 1 of the "Images in korg" program.
//! The design record is the handoff on that program (`korg:1128`); this crate
//! implements the half of it that has nothing to do with Postgres.
//!
//! **What lives here**: the display id (`img-<hex>`), decoding and probing an
//! uploaded byte string, generating the eager variants, hashing, and the
//! on-disk layout. **What deliberately does not**: the `attachment` node and
//! its edges (korg-core, where every node kind lives), the routes (korg-api)
//! and the tool surface (korg-mcp).
//!
//! That split is the answer to the handoff's D1. "A crate, not a
//! microservice" was decided to avoid a second deploy/auth/monitoring surface
//! while keeping a clean API boundary — and the boundary that is actually
//! clean is *bytes in, variants and paths out*. A crate that instead owned its
//! own migration, router and tool list would be a second architecture inside
//! one binary, and would not be any more extractable for it. This one is: it
//! knows nothing about korg except what an image is.
//!
//! Nothing in here talks to a database, a network, or a clock.

use std::fmt;

use sha2::{Digest, Sha256};

mod store;

pub use store::Store;

/// The multipart upload cap (handoff D6). 32 MB is far above any screenshot and
/// still below the point where holding one in memory to decode it matters.
pub const MAX_UPLOAD_BYTES: usize = 32 * 1024 * 1024;

/// Long edge of the `thumb` variant, in pixels — the inline/list rendering.
pub const THUMB_LONG_EDGE: u32 = 400;

/// Long edge of the `agent` variant, in pixels.
///
/// 1568 is not a round number and not a guess: it is the ceiling Anthropic
/// vision downscales to, so every pixel past it is bytes an agent pays for and
/// the model never sees (handoff D4).
pub const AGENT_LONG_EDGE: u32 = 1568;

/// JPEG quality for variants encoded lossily. Variants are for looking at, not
/// for archiving — the original is kept byte-exact for that.
const JPEG_QUALITY: u8 = 82;

/// What korg accepts an upload as. Sniffed from the bytes, never taken from the
/// client's `Content-Type`: a declared type is a claim, and the decoder is
/// going to have an opinion anyway.
const ACCEPTED: [image::ImageFormat; 5] = [
    image::ImageFormat::Png,
    image::ImageFormat::Jpeg,
    image::ImageFormat::Gif,
    image::ImageFormat::WebP,
    image::ImageFormat::Bmp,
];

#[derive(Debug, thiserror::Error)]
pub enum ImgError {
    /// The bytes are not an image korg accepts. Carries the accepted list, so
    /// the message doubles as the documentation a caller needs to retry.
    #[error("unsupported image: {0} — korg accepts PNG, JPEG, GIF, WebP and BMP")]
    Unsupported(String),
    #[error("could not decode image: {0}")]
    Decode(String),
    #[error("could not encode {variant} variant: {source}")]
    Encode {
        variant: &'static str,
        source: image::ImageError,
    },
    #[error("image is {size} bytes, over the {MAX_UPLOAD_BYTES}-byte upload cap")]
    TooLarge { size: usize },
    #[error("empty upload")]
    Empty,
    #[error("not an attachment id: {0} — expected the img-<hex> form, e.g. img-2c9")]
    BadId(String),
    #[error("no such variant: {0} — expected `thumb` or `agent`")]
    BadVariant(String),
    #[error("image store i/o at {path}: {source}")]
    Io {
        path: String,
        source: std::io::Error,
    },
}

// --- the display id ---------------------------------------------------------

/// An attachment's user-facing id: `img-<hex>`, where the hex **is** the node
/// id (handoff D3).
///
/// Deriving it from the node id rather than minting a separate sequence buys
/// uniqueness and comment-relatability for free, at the price of gaps in the
/// id space — which was the trade Ken accepted, because the gaps are invisible
/// and a second sequence is a second thing to keep consistent.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ImgId(i64);

impl ImgId {
    /// The id for a node. Node ids are positive by construction (`BIGSERIAL`),
    /// and a non-positive one is a caller bug rather than a user error.
    pub fn from_node_id(node_id: i64) -> Result<Self, ImgError> {
        if node_id <= 0 {
            return Err(ImgError::BadId(node_id.to_string()));
        }
        Ok(Self(node_id))
    }

    pub fn node_id(self) -> i64 {
        self.0
    }

    /// Parse `img-<hex>`, case-insensitively — the brainstorm wrote ids as
    /// `IMG-C2A` and the markdown token renders them lowercase, so both have to
    /// resolve to the same attachment or the two spellings are two bugs.
    pub fn parse(raw: &str) -> Result<Self, ImgError> {
        let bad = || ImgError::BadId(raw.to_string());
        let hex = raw
            .strip_prefix("img-")
            .or_else(|| raw.strip_prefix("IMG-"))
            .or_else(|| {
                raw.get(..4)
                    .filter(|p| p.eq_ignore_ascii_case("img-"))
                    .and(raw.get(4..))
            })
            .ok_or_else(bad)?;
        if hex.is_empty() {
            return Err(bad());
        }
        let node_id = i64::from_str_radix(hex, 16).map_err(|_| bad())?;
        Self::from_node_id(node_id)
    }
}

impl fmt::Display for ImgId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "img-{:x}", self.0)
    }
}

/// Where korg serves image bytes from. The one definition of the path shape —
/// korg-api registers it, korg-core reports it on every attachment read, and
/// `docs/api.md` documents it, so all three cannot disagree.
///
/// Paths are relative on purpose. korg has no configured public base URL (it is
/// reached as `kai`, `kubsdb`, `localhost` and a tailnet name depending on who
/// is asking), and a stored absolute URL would be wrong for most of them. A web
/// client resolves these against its own origin; an agent joins them onto the
/// same host it reaches korg's MCP endpoint on.
pub const ROUTE_PREFIX: &str = "/api/img";

impl ImgId {
    /// Path to the byte-exact original.
    pub fn url(self) -> String {
        format!("{ROUTE_PREFIX}/{self}")
    }

    /// Path to one generated variant.
    pub fn variant_url(self, variant: Variant) -> String {
        format!("{ROUTE_PREFIX}/{self}/{variant}")
    }
}

// --- variants ---------------------------------------------------------------

/// A generated size. The original is not one of these: it is kept byte-exact
/// and is addressed by the bare id.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Variant {
    /// ~400px long edge — the inline thumbnail and the attachment list.
    Thumb,
    /// ≤1568px long edge — what an agent fetches to read the image.
    Agent,
}

/// Both variants, in the order they are generated and reported.
pub const VARIANTS: [Variant; 2] = [Variant::Thumb, Variant::Agent];

impl Variant {
    pub fn as_str(self) -> &'static str {
        match self {
            Variant::Thumb => "thumb",
            Variant::Agent => "agent",
        }
    }

    /// Whether an original of these dimensions is **already** what this variant
    /// would be — it fits inside the box, so resizing it is a no-op (#1146).
    ///
    /// This is deliberately about pixels and nothing else, because it is the
    /// half of the rule a *reader* can evaluate: a caller resolving
    /// `/api/img/<id>/agent` has the original's dimensions and no idea what the
    /// encoder would have chosen. See [`prepare`] for the write-side rule,
    /// which is stricter.
    pub fn fits_original(self, width: u32, height: u32) -> bool {
        width.max(height) <= self.long_edge()
    }

    pub fn long_edge(self) -> u32 {
        match self {
            Variant::Thumb => THUMB_LONG_EDGE,
            Variant::Agent => AGENT_LONG_EDGE,
        }
    }

    pub fn parse(raw: &str) -> Result<Self, ImgError> {
        match raw {
            "thumb" => Ok(Variant::Thumb),
            "agent" => Ok(Variant::Agent),
            other => Err(ImgError::BadVariant(other.to_string())),
        }
    }
}

impl fmt::Display for Variant {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

// --- mime / extension -------------------------------------------------------

/// The file extension korg stores a blob of `mime` under, or `None` if that is
/// not a type korg produces. Used to rebuild a path from a stored mime, so it
/// must stay the inverse of what [`prepare`] records.
pub fn ext_for_mime(mime: &str) -> Option<&'static str> {
    match mime {
        "image/png" => Some("png"),
        "image/jpeg" => Some("jpg"),
        "image/gif" => Some("gif"),
        "image/webp" => Some("webp"),
        "image/bmp" => Some("bmp"),
        _ => None,
    }
}

fn mime_of(format: image::ImageFormat) -> &'static str {
    format.to_mime_type()
}

// --- preparing an upload ----------------------------------------------------

/// One generated variant: its metadata and its encoded bytes.
#[derive(Debug, Clone)]
pub struct PreparedVariant {
    pub variant: Variant,
    pub mime: &'static str,
    pub width: u32,
    pub height: u32,
    pub bytes: Vec<u8>,
}

/// Everything [`prepare`] learned about an upload, ready to be recorded as a
/// node and written to a [`Store`].
#[derive(Debug, Clone)]
pub struct Prepared {
    /// The sniffed type of the original — not what the client claimed.
    pub mime: &'static str,
    pub width: u32,
    pub height: u32,
    pub byte_size: u64,
    /// Lowercase hex SHA-256 of the original bytes. Recorded so a future
    /// "have we seen this before?" is answerable; deliberately NOT used to
    /// share a blob between attachments, because shared blobs would make the
    /// purge runbook (handoff D9) unable to promise it removed anything.
    pub content_hash: String,
    pub variants: Vec<PreparedVariant>,
}

impl Prepared {
    pub fn variant(&self, variant: Variant) -> Option<&PreparedVariant> {
        self.variants.iter().find(|v| v.variant == variant)
    }
}

/// Decode `bytes`, probe them, and generate every variant — the whole of the
/// eager-at-upload decision (handoff D4). No on-demand resizing exists, so this
/// is the only place pixels are ever touched.
///
/// EXIF stripping is not a step here and never appears as one: a variant is
/// encoded from decoded pixels, so the metadata is gone by construction. The
/// original keeps its bytes exactly, EXIF included — it is the archival copy.
/// The one `agent` variant that is *not* encoded (see [`skips_the_re_encode`])
/// is the original, EXIF and all.
///
/// A caller must therefore not assume every [`Variant`] appears in
/// [`Prepared::variants`]. An absent one means the original already satisfies
/// it — [`Variant::fits_original`] is how a reader re-derives that.
pub fn prepare(bytes: &[u8]) -> Result<Prepared, ImgError> {
    if bytes.is_empty() {
        return Err(ImgError::Empty);
    }
    if bytes.len() > MAX_UPLOAD_BYTES {
        return Err(ImgError::TooLarge { size: bytes.len() });
    }

    let format = image::guess_format(bytes)
        .map_err(|e| ImgError::Unsupported(e.to_string()))
        .and_then(|f| {
            ACCEPTED
                .contains(&f)
                .then_some(f)
                .ok_or_else(|| ImgError::Unsupported(f.to_mime_type().to_string()))
        })?;

    let image = image::load_from_memory_with_format(bytes, format)
        .map_err(|e| ImgError::Decode(e.to_string()))?;
    let (width, height) = (image.width(), image.height());

    // The variant format follows the *pixels*, not the source format: an image
    // with alpha must stay PNG or the transparency is silently flattened onto
    // whatever JPEG decides is white. Screenshots — the case this feature
    // exists for — land on PNG through that rule, and photographs on JPEG,
    // without either being special-cased by source type.
    let lossless = image.color().has_alpha();
    let mime = mime_of(format);

    let mut variants = Vec::with_capacity(VARIANTS.len());
    for variant in VARIANTS {
        let encoded = encode_variant(&image, variant, lossless)?;
        if supplanted_by_the_original(&encoded, bytes.len(), width, height) {
            continue;
        }
        variants.push(encoded);
    }

    Ok(Prepared {
        mime,
        width,
        height,
        byte_size: bytes.len() as u64,
        content_hash: format!("{:x}", Sha256::digest(bytes)),
        variants,
    })
}

/// #1146: when the re-encode came out no smaller than the original, throw it
/// away and let the original be the `agent` variant.
///
/// Found in the images program close-out: `img-46b`'s `:agent` variant was
/// 43,911 bytes against a 24,598-byte original at identical 1019x311
/// dimensions. Nothing was broken — the variant was a faithful re-encode of an
/// already-small PNG, and `image`'s encoder is simply worse at it than whatever
/// produced the paste. Every small screenshot was paying roughly double its
/// storage for a copy of itself.
///
/// Two conditions, and note which one is *measured*:
///
/// - **it fits** ([`Variant::fits_original`]) — otherwise the variant is a real
///   downscale, and it earns its bytes even when it has more of them, because
///   the point of `agent` is pixels the model will actually look at.
/// - **it is not smaller** — checked against the encoded bytes rather than
///   predicted from the mime type.
///
/// The prediction was the first shape of this fix and it was wrong. "Same mime
/// family, so the re-encode is redundant" misses the case korg exists for: an
/// **opaque** screenshot PNG encodes to JPEG, a different family, and JPEG at
/// q82 loses badly to PNG on flat UI colour — a synthetic 800x600 gradient
/// measured 42,193 bytes of JPEG against 9,878 of PNG. Predicting by type kept
/// the bigger file precisely where the bug bites hardest. Encoding and then
/// looking costs one throwaway encode of an image already under 1568px, which
/// is milliseconds, and it cannot be wrong about the thing it is measuring.
///
/// Only `agent` is ever reached: `fits_original` is asked of the agent ceiling,
/// and #1146 scoped it there deliberately — `thumb` is 400px, so an original
/// small enough to qualify is tiny either way, and the thumb is what the UI
/// renders everywhere.
///
/// The cost, stated because it is real: the `agent` URL now serves the
/// original's bytes, so it carries whatever EXIF the original carried. See
/// [`encode_variant`] — that stripping was always incidental, and the original
/// has been served EXIF-and-all at `/api/img/<id>` since the feature shipped,
/// so this exposes nothing a caller could not already fetch.
fn supplanted_by_the_original(
    encoded: &PreparedVariant,
    original_bytes: usize,
    width: u32,
    height: u32,
) -> bool {
    encoded.variant == Variant::Agent
        && Variant::Agent.fits_original(width, height)
        && encoded.bytes.len() >= original_bytes
}

/// Resize (never upscale) and encode one variant.
fn encode_variant(
    image: &image::DynamicImage,
    variant: Variant,
    lossless: bool,
) -> Result<PreparedVariant, ImgError> {
    let edge = variant.long_edge();
    // `resize` fits inside the box and preserves the aspect ratio, so passing
    // the long edge for both dimensions is how you say "long edge = N".
    let resized = if image.width().max(image.height()) > edge {
        Some(image.resize(edge, edge, image::imageops::FilterType::Lanczos3))
    } else {
        // Already small enough, and still re-encoded rather than copied.
        //
        // This used to be justified as "a variant that is sometimes the
        // original's bytes and sometimes not is a variant with two contracts".
        // #1146 accepted the second contract for `agent` — see
        // `skips_the_re_encode`, which decides that before we are ever called —
        // because the measured cost of the tidier rule was double storage on
        // every small paste. What reaches here is a re-encode that is worth
        // doing: a `thumb`, or an `agent` whose type is genuinely changing.
        //
        // EXIF stripping was always a side effect of this branch rather than a
        // purpose of it, so a skipped `agent` keeps the original's metadata.
        None
    };
    let source = resized.as_ref().unwrap_or(image);

    let mut bytes = Vec::new();
    let mime = if lossless {
        source
            .write_to(
                &mut std::io::Cursor::new(&mut bytes),
                image::ImageFormat::Png,
            )
            .map_err(|source| ImgError::Encode {
                variant: variant.as_str(),
                source,
            })?;
        "image/png"
    } else {
        use image::ImageEncoder;
        let rgb = source.to_rgb8();
        image::codecs::jpeg::JpegEncoder::new_with_quality(&mut bytes, JPEG_QUALITY)
            .write_image(
                rgb.as_raw(),
                rgb.width(),
                rgb.height(),
                image::ExtendedColorType::Rgb8,
            )
            .map_err(|source| ImgError::Encode {
                variant: variant.as_str(),
                source,
            })?;
        "image/jpeg"
    };

    Ok(PreparedVariant {
        variant,
        mime,
        width: source.width(),
        height: source.height(),
        bytes,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A tiny RGBA PNG, wide enough that both variants have to downscale.
    fn png(width: u32, height: u32) -> Vec<u8> {
        let mut buf = image::RgbaImage::new(width, height);
        for (x, y, px) in buf.enumerate_pixels_mut() {
            *px = image::Rgba([(x % 256) as u8, (y % 256) as u8, 128, 255]);
        }
        let mut out = Vec::new();
        image::DynamicImage::ImageRgba8(buf)
            .write_to(&mut std::io::Cursor::new(&mut out), image::ImageFormat::Png)
            .expect("encode test png");
        out
    }

    /// A PNG with **no** alpha channel. Its variants encode as JPEG, which
    /// makes it the case `skips_the_re_encode` must NOT skip.
    fn opaque_png(width: u32, height: u32) -> Vec<u8> {
        let buf = image::RgbImage::from_fn(width, height, |x, y| {
            image::Rgb([(x % 256) as u8, (y % 256) as u8, 128])
        });
        let mut out = Vec::new();
        image::DynamicImage::ImageRgb8(buf)
            .write_to(&mut std::io::Cursor::new(&mut out), image::ImageFormat::Png)
            .unwrap();
        out
    }

    /// An opaque PNG of pseudo-random noise: incompressible for PNG, exactly
    /// what JPEG's lossy coding is good at. The mirror image of `opaque_png`.
    fn noisy_png(width: u32, height: u32) -> Vec<u8> {
        let buf = image::RgbImage::from_fn(width, height, |x, y| {
            let n = x.wrapping_mul(2_654_435_761) ^ y.wrapping_mul(40_503);
            image::Rgb([(n >> 3) as u8, (n >> 11) as u8, (n >> 19) as u8])
        });
        let mut out = Vec::new();
        image::DynamicImage::ImageRgb8(buf)
            .write_to(&mut std::io::Cursor::new(&mut out), image::ImageFormat::Png)
            .unwrap();
        out
    }

    fn jpeg(width: u32, height: u32) -> Vec<u8> {
        let buf = image::RgbImage::new(width, height);
        let mut out = Vec::new();
        image::DynamicImage::ImageRgb8(buf)
            .write_to(
                &mut std::io::Cursor::new(&mut out),
                image::ImageFormat::Jpeg,
            )
            .expect("encode test jpeg");
        out
    }

    #[test]
    fn an_id_round_trips_through_its_hex_spelling() {
        let id = ImgId::from_node_id(3114).unwrap();
        assert_eq!(id.to_string(), "img-c2a");
        assert_eq!(ImgId::parse("img-c2a").unwrap(), id);
        // The brainstorm wrote them uppercase; both spellings are the same id.
        assert_eq!(ImgId::parse("IMG-C2A").unwrap(), id);
        assert_eq!(ImgId::parse("Img-C2a").unwrap(), id);
    }

    #[test]
    fn a_malformed_id_is_refused_rather_than_coerced() {
        for raw in ["", "c2a", "img-", "img-zz", "img--1", "1081"] {
            assert!(ImgId::parse(raw).is_err(), "{raw:?} should not parse");
        }
        assert!(ImgId::from_node_id(0).is_err());
        assert!(ImgId::from_node_id(-3).is_err());
    }

    #[test]
    fn every_stored_mime_has_an_extension() {
        // `ext_for_mime` rebuilds a path from a stored mime, so a format
        // `prepare` can record but it cannot spell is an unreadable blob.
        for format in ACCEPTED {
            assert!(
                ext_for_mime(format.to_mime_type()).is_some(),
                "no extension for {}",
                format.to_mime_type()
            );
        }
        assert!(ext_for_mime("image/tiff").is_none());
    }

    #[test]
    fn variants_are_generated_eagerly_and_never_upscale() {
        let prepared = prepare(&png(2000, 1000)).expect("prepare");
        assert_eq!(prepared.mime, "image/png");
        assert_eq!((prepared.width, prepared.height), (2000, 1000));
        assert_eq!(prepared.variants.len(), VARIANTS.len());

        let thumb = prepared.variant(Variant::Thumb).unwrap();
        assert_eq!(thumb.width, THUMB_LONG_EDGE, "long edge is the thumb bound");
        assert_eq!(thumb.height, THUMB_LONG_EDGE / 2, "aspect ratio preserved");

        let agent = prepared.variant(Variant::Agent).unwrap();
        assert_eq!(
            agent.width, AGENT_LONG_EDGE,
            "the agent variant is capped at Anthropic's vision ceiling"
        );

        // An image already inside a bound keeps its size rather than being
        // blown up to fill it.
        let small = prepare(&png(120, 90)).expect("prepare");
        let thumb = small.variant(Variant::Thumb).unwrap();
        assert_eq!((thumb.width, thumb.height), (120, 90), "thumb upscaled");
        assert!(
            small.variant(Variant::Agent).is_none(),
            "and at 120x90 the agent variant is the original itself (#1146)"
        );
    }

    #[test]
    fn alpha_decides_the_variant_format_not_the_source_format() {
        // Both samples are over the agent ceiling on purpose, so every variant
        // is really encoded and the format rule is what is under test rather
        // than #1146's skip.
        //
        // Screenshots arrive as PNG-with-alpha and must not be flattened.
        let with_alpha = prepare(&png(2000, 50)).unwrap();
        for variant in VARIANTS {
            assert_eq!(with_alpha.variant(variant).unwrap().mime, "image/png");
        }
        // Photographs have no alpha to lose, and JPEG is a lot smaller.
        let without = prepare(&jpeg(2000, 50)).unwrap();
        assert_eq!(without.mime, "image/jpeg");
        for variant in VARIANTS {
            assert_eq!(without.variant(variant).unwrap().mime, "image/jpeg");
        }
    }

    // === #1146: an agent variant that would only be a bigger copy ===========

    /// The bug, in the shape it was found: an already-small PNG whose `:agent`
    /// re-encode came out *larger* than the original it was made from.
    ///
    /// `img-46b` on #1115 measured 43,911 bytes of variant against a 24,598-byte
    /// original at identical 1019x311 dimensions. The fix is to notice and skip,
    /// so the assertion is about the variant's absence — and about the reason
    /// being derivable from the original alone, which is what the serve path has.
    #[test]
    fn an_agent_variant_that_could_only_be_a_bigger_copy_is_not_written() {
        let prepared = prepare(&png(1019, 311)).expect("prepare");

        assert!(
            prepared.variant(Variant::Agent).is_none(),
            "the original is inside the vision ceiling and the re-encode
             measured no smaller, so it is a second copy of the same pixels
             that costs more to keep"
        );
        assert!(
            prepared.variant(Variant::Thumb).is_some(),
            "the thumb is a real downscale and is untouched by #1146"
        );
        assert!(
            Variant::Agent.fits_original(prepared.width, prepared.height),
            "and a reader holding only the original's dimensions can work out              why the variant is missing — that is how the agent URL still              resolves"
        );
    }

    /// The case that killed the first version of this fix, kept as its fence.
    ///
    /// An **opaque** PNG has no alpha, so its variants encode as JPEG — a
    /// different mime family. The first rule read "same mime family, so the
    /// re-encode is redundant" and would have kept this variant on the grounds
    /// that a conversion is real work. It is real work that makes the file four
    /// times bigger: JPEG at q82 is poor at flat synthetic colour, which is what
    /// UI screenshots are made of. Measuring gets this right; predicting from
    /// the type keeps the larger file exactly where the bug bites hardest.
    #[test]
    fn a_bigger_re_encode_is_dropped_even_when_it_changes_the_type() {
        let prepared = prepare(&opaque_png(800, 600)).expect("prepare");
        assert_eq!(prepared.mime, "image/png", "no alpha, but still a PNG");

        assert!(
            prepared.variant(Variant::Agent).is_none(),
            "the JPEG conversion is larger than the PNG it came from, so the
             original is the better agent variant despite the type change"
        );
    }

    /// The converse, so the rule cannot be read as "small originals never get a
    /// variant": a re-encode that genuinely shrinks is kept even though the
    /// original fits. It is about bytes, not about fitting.
    #[test]
    fn a_smaller_re_encode_is_kept_even_though_the_original_fits() {
        let prepared = prepare(&noisy_png(800, 600)).expect("prepare");

        let agent = prepared
            .variant(Variant::Agent)
            .expect("photographic noise is what JPEG is good at");
        assert_eq!(agent.mime, "image/jpeg");
        assert_eq!(
            (agent.width, agent.height),
            (800, 600),
            "no downscale — this variant earns its place on bytes alone"
        );
        assert!(
            (agent.bytes.len() as u64) < prepared.byte_size,
            "{} should beat {}",
            agent.bytes.len(),
            prepared.byte_size
        );
    }

    /// A JPEG small enough to fit skips too: re-encoding an already-lossy JPEG
    /// at q82 is generation loss that also fails to save anything.
    #[test]
    fn an_already_lossy_original_is_not_re_encoded_either() {
        let prepared = prepare(&jpeg(300, 200)).expect("prepare");
        assert_eq!(prepared.mime, "image/jpeg");
        assert!(prepared.variant(Variant::Agent).is_none());
    }

    /// And an image over the ceiling is unaffected, whatever its type: that
    /// variant is a real downscale and is the whole point of the feature.
    #[test]
    fn an_oversized_original_still_gets_its_agent_variant() {
        let prepared = prepare(&png(3000, 100)).expect("prepare");
        let agent = prepared.variant(Variant::Agent).expect("a real downscale");
        assert_eq!(agent.width, AGENT_LONG_EDGE);
        assert!(!Variant::Agent.fits_original(prepared.width, prepared.height));
    }

    #[test]
    fn the_content_hash_is_of_the_original_bytes() {
        let bytes = png(40, 40);
        let prepared = prepare(&bytes).unwrap();
        assert_eq!(
            prepared.content_hash,
            format!("{:x}", Sha256::digest(&bytes))
        );
        assert_eq!(prepared.byte_size, bytes.len() as u64);
        // Same bytes, same hash — which is all the hash promises. korg records
        // it and does NOT act on it: two identical uploads are two blobs.
        assert_eq!(prepare(&bytes).unwrap().content_hash, prepared.content_hash);
    }

    #[test]
    fn non_images_and_oversized_uploads_are_refused() {
        assert!(matches!(prepare(b""), Err(ImgError::Empty)));
        assert!(matches!(
            prepare(b"this is a text file, not a screenshot"),
            Err(ImgError::Unsupported(_))
        ));
        assert!(matches!(
            prepare(&vec![0u8; MAX_UPLOAD_BYTES + 1]),
            Err(ImgError::TooLarge { .. })
        ));
    }

    #[test]
    fn urls_are_relative_and_built_from_one_prefix() {
        let id = ImgId::from_node_id(3114).unwrap();
        assert_eq!(id.url(), "/api/img/img-c2a");
        assert_eq!(id.variant_url(Variant::Thumb), "/api/img/img-c2a/thumb");
        assert_eq!(id.variant_url(Variant::Agent), "/api/img/img-c2a/agent");
        assert!(
            id.url().starts_with(ROUTE_PREFIX) && !id.url().contains("://"),
            "korg has no configured public base URL — these must stay relative"
        );
    }

    #[test]
    fn variant_names_round_trip() {
        for variant in VARIANTS {
            assert_eq!(Variant::parse(variant.as_str()).unwrap(), variant);
        }
        assert!(Variant::parse("original").is_err());
        assert!(Variant::parse("").is_err());
    }
}
