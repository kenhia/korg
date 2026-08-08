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

    let mut variants = Vec::with_capacity(VARIANTS.len());
    for variant in VARIANTS {
        variants.push(encode_variant(&image, variant, lossless)?);
    }

    Ok(Prepared {
        mime: mime_of(format),
        width,
        height,
        byte_size: bytes.len() as u64,
        content_hash: format!("{:x}", Sha256::digest(bytes)),
        variants,
    })
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
        // Already small enough. It is still re-encoded rather than copied —
        // that is what strips the EXIF, and a variant that is sometimes the
        // original's bytes and sometimes not is a variant with two contracts.
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
        for variant in VARIANTS {
            let v = small.variant(variant).unwrap();
            assert_eq!((v.width, v.height), (120, 90), "{variant} upscaled");
        }
    }

    #[test]
    fn alpha_decides_the_variant_format_not_the_source_format() {
        // Screenshots arrive as PNG-with-alpha and must not be flattened.
        let with_alpha = prepare(&png(50, 50)).unwrap();
        for variant in VARIANTS {
            assert_eq!(with_alpha.variant(variant).unwrap().mime, "image/png");
        }
        // Photographs have no alpha to lose, and JPEG is a lot smaller.
        let without = prepare(&jpeg(50, 50)).unwrap();
        assert_eq!(without.mime, "image/jpeg");
        for variant in VARIANTS {
            assert_eq!(without.variant(variant).unwrap().mime, "image/jpeg");
        }
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
