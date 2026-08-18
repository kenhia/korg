//! The on-disk blob store: one directory per attachment, under a bucketed root.
//!
//! Layout (handoff's "per-attachment dir on disk", shape left to the
//! implementer):
//!
//! ```text
//! <root>/0003/img-c2a/original.png
//!                     thumb.png
//!                     agent.png
//! ```
//!
//! **One directory per attachment** is the load-bearing part: slice 3's
//! sensitive-image purge runbook (handoff D9) has to be able to promise it
//! removed every copy, and "remove this directory" is a promise a person can
//! check. It is also why blobs are never shared between attachments even when
//! their content hashes match (D4) — a shared blob turns that promise into an
//! audit.
//!
//! **The bucket** is `node_id / 1000`, so a store holding Ken's estimated ~30K
//! captures has thirty directories of a thousand rather than one of thirty
//! thousand. Sequential node ids fill buckets in order, which also makes the
//! directory listing readable as a rough timeline.

use std::path::{Path, PathBuf};

use crate::{ext_for_mime, ImgError, ImgId, Prepared, Variant};

/// A rooted image store. Cheap to clone; holds no handles.
#[derive(Debug, Clone)]
pub struct Store {
    root: PathBuf,
}

impl Store {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Create the root if it is not there. Called at startup so a
    /// misconfigured mount is a log line then rather than a failed paste later.
    pub fn ensure_root(&self) -> Result<(), ImgError> {
        std::fs::create_dir_all(&self.root).map_err(|source| self.io(&self.root, source))
    }

    /// The directory holding every blob for one attachment.
    pub fn dir(&self, id: ImgId) -> PathBuf {
        self.root
            .join(format!("{:04}", id.node_id() / 1000))
            .join(id.to_string())
    }

    /// Path to the byte-exact original.
    pub fn original_path(&self, id: ImgId, mime: &str) -> Result<PathBuf, ImgError> {
        Ok(self.dir(id).join(format!("original.{}", ext(mime)?)))
    }

    /// Path to a generated variant.
    pub fn variant_path(
        &self,
        id: ImgId,
        variant: Variant,
        mime: &str,
    ) -> Result<PathBuf, ImgError> {
        Ok(self.dir(id).join(format!("{variant}.{}", ext(mime)?)))
    }

    /// Write the original and every prepared variant.
    ///
    /// All-or-nothing: a partial write is removed rather than left for a later
    /// read to trip over, because a half-written attachment is exactly the
    /// thing the "existence is never parsed from prose" rule (D2) cannot
    /// protect against — the row would say the blob is there.
    pub fn write(&self, id: ImgId, prepared: &Prepared, original: &[u8]) -> Result<(), ImgError> {
        match self.write_inner(id, prepared, original) {
            Ok(()) => Ok(()),
            Err(e) => {
                // Best effort: if this fails too, the caller's error is still
                // the one worth reporting, and the sweeper's edge-less rule
                // will collect the row.
                let _ = std::fs::remove_dir_all(self.dir(id));
                Err(e)
            }
        }
    }

    fn write_inner(&self, id: ImgId, prepared: &Prepared, original: &[u8]) -> Result<(), ImgError> {
        let dir = self.dir(id);
        std::fs::create_dir_all(&dir).map_err(|source| self.io(&dir, source))?;

        let path = self.original_path(id, prepared.mime)?;
        std::fs::write(&path, original).map_err(|source| self.io(&path, source))?;

        for variant in &prepared.variants {
            let path = self.variant_path(id, variant.variant, variant.mime)?;
            std::fs::write(&path, &variant.bytes).map_err(|source| self.io(&path, source))?;
        }
        Ok(())
    }

    /// Read one blob back. `None` when the file is absent — which a caller must
    /// distinguish from an error, because "the row exists but the blob does
    /// not" is a real state after a restore that missed the volume, and it
    /// deserves its own message rather than a 500.
    pub fn read(&self, path: &Path) -> Result<Option<Vec<u8>>, ImgError> {
        match std::fs::read(path) {
            Ok(bytes) => Ok(Some(bytes)),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(source) => Err(self.io(path, source)),
        }
    }

    /// Remove every blob for an attachment. `false` when there was nothing
    /// there, which is not an error: a discard after a failed write is the
    /// ordinary way to reach it.
    pub fn remove(&self, id: ImgId) -> Result<bool, ImgError> {
        let dir = self.dir(id);
        match std::fs::remove_dir_all(&dir) {
            Ok(()) => Ok(true),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(false),
            Err(source) => Err(self.io(&dir, source)),
        }
    }

    fn io(&self, path: &Path, source: std::io::Error) -> ImgError {
        ImgError::Io {
            path: path.display().to_string(),
            source,
        }
    }
}

fn ext(mime: &str) -> Result<&'static str, ImgError> {
    ext_for_mime(mime).ok_or_else(|| ImgError::Unsupported(mime.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::prepare;

    fn temp_root(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "korg-img-{name}-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    fn sample() -> Vec<u8> {
        let buf = image::RgbaImage::from_pixel(64, 32, image::Rgba([10, 20, 30, 255]));
        let mut out = Vec::new();
        image::DynamicImage::ImageRgba8(buf)
            .write_to(&mut std::io::Cursor::new(&mut out), image::ImageFormat::Png)
            .unwrap();
        out
    }

    #[test]
    fn a_write_lands_every_blob_in_one_directory_per_attachment() {
        let root = temp_root("write");
        let store = Store::new(&root);
        store.ensure_root().unwrap();

        let id = ImgId::from_node_id(3114).unwrap();
        let bytes = sample();
        let prepared = prepare(&bytes).unwrap();
        store.write(id, &prepared, &bytes).unwrap();

        // The bucket is derived, not stored, so this pins the layout the purge
        // runbook will be written against.
        assert_eq!(store.dir(id), root.join("0003").join("img-c2a"));

        let original = store.original_path(id, prepared.mime).unwrap();
        assert_eq!(
            store.read(&original).unwrap().as_deref(),
            Some(bytes.as_slice()),
            "the original is kept byte-exact"
        );
        for v in &prepared.variants {
            let path = store.variant_path(id, v.variant, v.mime).unwrap();
            assert_eq!(store.read(&path).unwrap().as_deref(), Some(&v.bytes[..]));
        }

        // The store writes what `prepare` produced and nothing else — which
        // since #1146 is not always every variant. This 64x32 sample is well
        // inside the agent ceiling and re-encodes no smaller, so its agent
        // variant IS the original and no second blob is written for it. The
        // absence is the storage win, so it is asserted rather than assumed.
        assert!(
            prepared.variant(Variant::Agent).is_none(),
            "a 64x32 paste needs no agent variant"
        );
        let mut names: Vec<String> = std::fs::read_dir(store.dir(id))
            .unwrap()
            .flatten()
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        assert_eq!(
            names,
            ["original.png", "thumb.png"],
            "no agent.png duplicating the original beside it"
        );

        // Two attachments never share a directory, whatever their content.
        let other = ImgId::from_node_id(3115).unwrap();
        assert_ne!(store.dir(id), store.dir(other));

        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn a_missing_blob_reads_as_absent_rather_than_as_an_error() {
        let root = temp_root("missing");
        let store = Store::new(&root);
        let id = ImgId::from_node_id(7).unwrap();
        let path = store.original_path(id, "image/png").unwrap();
        assert!(store.read(&path).unwrap().is_none());
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn remove_is_idempotent_and_takes_the_whole_attachment() {
        let root = temp_root("remove");
        let store = Store::new(&root);
        store.ensure_root().unwrap();
        let id = ImgId::from_node_id(42).unwrap();
        let bytes = sample();
        store.write(id, &prepare(&bytes).unwrap(), &bytes).unwrap();

        assert!(
            store.remove(id).unwrap(),
            "first removal took the directory"
        );
        assert!(!store.dir(id).exists());
        assert!(
            !store.remove(id).unwrap(),
            "a second discard is not an error — it is how a failed write is cleaned up"
        );
        std::fs::remove_dir_all(&root).ok();
    }
}
