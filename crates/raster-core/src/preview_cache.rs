//! Bounded, content-versioned PNG cache. Caller serializes access; no disk cache.
use std::{
    collections::VecDeque,
    path::{Path, PathBuf},
    sync::Arc,
    time::SystemTime,
};

#[derive(PartialEq, Eq)]
struct Key {
    path: PathBuf,
    edge: u32,
    len: u64,
    modified: SystemTime,
    avif_hint: bool,
    #[cfg(unix)]
    identity: (u64, u64, i64, i64),
}
impl Key {
    fn read(path: &Path, edge: u32) -> Result<Self, String> {
        let canonical = path.canonicalize().map_err(super::err)?;
        let metadata = canonical.metadata().map_err(super::err)?;
        #[cfg(unix)]
        use std::os::unix::fs::MetadataExt;
        Ok(Self {
            path: canonical,
            edge,
            len: metadata.len(),
            modified: metadata.modified().map_err(super::err)?,
            avif_hint: path
                .extension()
                .is_some_and(|e| e.eq_ignore_ascii_case("avif")),
            #[cfg(unix)]
            identity: (
                metadata.dev(),
                metadata.ino(),
                metadata.ctime(),
                metadata.ctime_nsec(),
            ),
        })
    }
}
pub struct PreviewImage {
    pub png: Vec<u8>,
    pub width: u32,
    pub height: u32,
}
struct Entry {
    key: Key,
    image: Arc<PreviewImage>,
}
pub struct PreviewCache {
    entries: VecDeque<Entry>,
    bytes: usize,
    limit: usize,
}
impl PreviewCache {
    pub fn new(max_bytes: usize) -> Self {
        Self {
            entries: VecDeque::new(),
            bytes: 0,
            limit: max_bytes,
        }
    }
    pub fn cached_bytes(&self) -> usize {
        self.bytes
    }
    pub fn get(&mut self, path: &Path, edge: u32) -> Result<Arc<PreviewImage>, String> {
        if !(64..=1600).contains(&edge) {
            return Err("Preview size must be 64..1600 pixels".into());
        }
        let key = Key::read(path, edge)?;
        if let Some(index) = self.entries.iter().position(|e| e.key == key) {
            let entry = self.entries.remove(index).unwrap();
            let image = Arc::clone(&entry.image);
            self.entries.push_back(entry);
            return Ok(image);
        }
        // Remove outdated versions of this file, including its other preview sizes.
        self.entries.retain(|entry| {
            let keep = entry.key.path != key.path
                || (entry.key.len == key.len && entry.key.modified == key.modified && {
                    #[cfg(unix)]
                    {
                        entry.key.identity == key.identity
                    }
                    #[cfg(not(unix))]
                    {
                        true
                    }
                });
            if !keep {
                self.bytes -= entry.image.png.len();
            }
            keep
        });
        let (png, width, height) = super::preview_png(path, edge)?;
        let image = Arc::new(PreviewImage { png, width, height });
        if Key::read(path, edge)? != key {
            return Err("Image changed during preview; please retry".into());
        }
        let size = image.png.len();
        if size <= self.limit {
            while self.bytes + size > self.limit || self.entries.len() >= 32 {
                if let Some(entry) = self.entries.pop_front() {
                    self.bytes -= entry.image.png.len();
                } else {
                    break;
                }
            }
            self.bytes += size;
            self.entries.push_back(Entry {
                key,
                image: Arc::clone(&image),
            });
        }
        Ok(image)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn exact_hits_invalidations_and_byte_limit() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("photo.png");
        let save = |c| {
            image::RgbImage::from_pixel(128, 96, image::Rgb([c, 80, 100]))
                .save(&path)
                .unwrap()
        };
        save(20);
        let mut cache = PreviewCache::new(1024 * 1024);
        let a = cache.get(&path, 64).unwrap();
        assert!(Arc::ptr_eq(&a, &cache.get(&path, 64).unwrap()));
        assert_eq!(a.png, super::super::preview_png(&path, 64).unwrap().0);
        save(200);
        let b = cache.get(&path, 64).unwrap();
        assert_ne!(a.png, b.png);
        assert_eq!(cache.entries.len(), 1);
        let mut tiny = PreviewCache::new(1);
        assert_eq!(tiny.get(&path, 64).unwrap().png, b.png);
        assert_eq!(tiny.cached_bytes(), 0);
        std::fs::remove_file(path).unwrap();
        assert!(cache.get(&temp.path().join("photo.png"), 64).is_err());
    }
}
