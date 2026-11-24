//! The images a rendering shows, re-encoded (finding C-4 of the stage 15 review, decision 0046).
//!
//! `mail_content::render_html` shows an image only as its re-encoding; a first rendering lists
//! the images it needs (`RenderedHtml::pending_images`). [`Images`] re-encodes them in one batch
//! per rendering: in the `mail-mcp` helper when the shell passed one (the product), under the
//! limits of attachment previews, else on the blocking pool (tests and development builds
//! without the helper binary; a release build always passes the path and refuses the images
//! when the file is missing). The app's process never decodes them.
//!
//! Results are kept in memory by a keyed hash of the original bytes (at most [`CACHE_BYTES`] of
//! data URLs and [`CACHE_ENTRIES`] entries, the least recently used go first), so a message pays
//! for its images the first time it is shown. Refusals are kept as well (a hostile image is not
//! tried again on every view); a helper that could not start is not.

use std::collections::HashMap;
use std::collections::hash_map::RandomState;
use std::hash::BuildHasher;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, PoisonError};

use mail_content::{ImageBytes, ReencodedImage, ReencodedImages};
use mail_model::{CoreError, PreviewError};

use super::blocking;

/// Most bytes of data URLs kept.
pub(crate) const CACHE_BYTES: usize = 64 * 1024 * 1024;

/// Most images kept (re-encoded or refused).
pub(crate) const CACHE_ENTRIES: usize = 4096;

/// What a refusal costs in the cache's byte count.
const REFUSAL_BYTES: usize = 64;

/// The key of an image: two keyed SipHash values of its bytes and its length. The keys are
/// random per process, so no mail can be made to collide with another image.
type Key = (u64, u64, usize);

struct Entry {
    data_url: Option<Arc<str>>,
    used: u64,
}

impl Entry {
    fn size(&self) -> usize {
        self.data_url
            .as_ref()
            .map_or(REFUSAL_BYTES, |url| url.len())
    }
}

struct Cache {
    hashers: (RandomState, RandomState),
    entries: HashMap<Key, Entry>,
    bytes: usize,
    clock: u64,
}

impl Cache {
    fn new() -> Self {
        Self {
            hashers: (RandomState::new(), RandomState::new()),
            entries: HashMap::new(),
            bytes: 0,
            clock: 0,
        }
    }

    fn key(&self, bytes: &[u8]) -> Key {
        (
            self.hashers.0.hash_one(bytes),
            self.hashers.1.hash_one(bytes),
            bytes.len(),
        )
    }

    /// `Some(Some(url))` re-encoded, `Some(None)` refused, `None` not kept.
    fn get(&mut self, key: &Key) -> Option<Option<Arc<str>>> {
        self.clock += 1;
        let clock = self.clock;
        self.entries.get_mut(key).map(|entry| {
            entry.used = clock;
            entry.data_url.clone()
        })
    }

    fn put(&mut self, key: Key, data_url: Option<Arc<str>>) {
        self.clock += 1;
        let entry = Entry {
            data_url,
            used: self.clock,
        };
        if entry.size() > CACHE_BYTES / 4 {
            return;
        }
        self.bytes += entry.size();
        if let Some(old) = self.entries.insert(key, entry) {
            self.bytes -= old.size();
        }
        while self.bytes > CACHE_BYTES || self.entries.len() > CACHE_ENTRIES {
            let Some(oldest) = self
                .entries
                .iter()
                .min_by_key(|(_, entry)| entry.used)
                .map(|(key, _)| *key)
            else {
                break;
            };
            if let Some(old) = self.entries.remove(&oldest) {
                self.bytes -= old.size();
            }
        }
    }
}

/// Re-encodes the images of renderings and keeps the results (see the module docs).
pub(crate) struct Images {
    helper: Option<PathBuf>,
    cache: Mutex<Cache>,
    /// Batches re-encoded (helper calls, or runs in this process without a helper).
    batches: AtomicUsize,
}

impl Images {
    pub(crate) fn new(helper: Option<PathBuf>) -> Self {
        Self {
            helper,
            cache: Mutex::new(Cache::new()),
            batches: AtomicUsize::new(0),
        }
    }

    fn cache(&self) -> std::sync::MutexGuard<'_, Cache> {
        self.cache.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// The re-encodings of `images` that are kept already; nothing is re-encoded.
    pub(crate) fn known<'a>(
        &self,
        images: impl IntoIterator<Item = &'a ImageBytes>,
    ) -> ReencodedImages {
        let mut known = ReencodedImages::new();
        let mut cache = self.cache();
        for image in images {
            let key = cache.key(image);
            match cache.get(&key) {
                Some(Some(url)) => {
                    known.insert(image.clone(), url);
                }
                Some(None) => known.refuse(image.clone()),
                None => {}
            }
        }
        known
    }

    /// The re-encodings of `images`: kept ones from the cache, the others in one batch. Every
    /// image is in the result, re-encoded or refused.
    pub(crate) async fn reencode(&self, images: Vec<ImageBytes>) -> ReencodedImages {
        let mut result = ReencodedImages::new();
        let mut missing: Vec<(Key, ImageBytes)> = Vec::new();
        {
            let mut cache = self.cache();
            for image in images {
                let key = cache.key(&image);
                if missing.iter().any(|(k, _)| *k == key) {
                    continue;
                }
                match cache.get(&key) {
                    Some(Some(url)) => {
                        result.insert(image, url);
                    }
                    Some(None) => result.refuse(image),
                    None => missing.push((key, image)),
                }
            }
        }
        if missing.is_empty() {
            return result;
        }
        let originals: Vec<ImageBytes> = missing.iter().map(|(_, image)| image.clone()).collect();
        self.batches.fetch_add(1, Ordering::Relaxed);
        let outcome = self.run_batch(originals).await;
        let mut cache = self.cache();
        match outcome {
            Ok(results) => {
                for ((key, original), image) in missing.into_iter().zip(results) {
                    let url = match image {
                        Ok(image) => {
                            let url: Arc<str> = image.data_url.into();
                            result.insert(original, url.clone()).then_some(url)
                        }
                        Err(err) => {
                            tracing::debug!(error = %err, "an image of a mail is not shown");
                            result.refuse(original);
                            None
                        }
                    };
                    cache.put(key, url);
                }
            }
            Err(err) => {
                // The batch failed as a whole: none of its images is shown. A helper that ran
                // and was stopped or crashed is not asked about them again; one that could not
                // start (`Unsupported`) may work next time.
                let keep = !matches!(err, PreviewError::Unsupported(_));
                tracing::warn!(error = %err, images = missing.len(), "re-encoding the images of a mail failed");
                for (key, original) in missing {
                    result.refuse(original);
                    if keep {
                        cache.put(key, None);
                    }
                }
            }
        }
        result
    }

    async fn run_batch(
        &self,
        images: Vec<ImageBytes>,
    ) -> Result<Vec<Result<ReencodedImage, PreviewError>>, PreviewError> {
        match &self.helper {
            Some(helper) => {
                mail_mcp::preview::reencode_isolated(
                    helper,
                    &images,
                    mail_mcp::preview::PreviewLimits::default(),
                )
                .await
            }
            None => blocking(move || mail_content::reencode_images(&images))
                .await
                .map_err(|err: CoreError| PreviewError::Damaged(err.to_string())),
        }
    }

    /// Batches re-encoded so far (for tests: a cache hit runs none).
    #[cfg(test)]
    pub(crate) fn batches(&self) -> usize {
        self.batches.load(Ordering::Relaxed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn url(n: usize) -> Arc<str> {
        Arc::from(format!("data:image/png;base64,{}", "A".repeat(n)))
    }

    #[test]
    fn the_cache_keeps_the_recently_used_within_its_bounds() {
        let mut cache = Cache::new();
        let a = cache.key(b"a");
        let b = cache.key(b"b");
        assert_ne!(a, b);
        assert_eq!(a, cache.key(b"a"));
        assert_ne!(
            a,
            Cache::new().key(b"a"),
            "keys are random per cache, not guessable"
        );
        let big = CACHE_BYTES / 4 - 64;
        cache.put(a, Some(url(big)));
        cache.put(b, Some(url(big)));
        cache.put(cache.key(b"c"), None);
        assert!(cache.get(&a).is_some(), "a used again");
        cache.put(cache.key(b"d"), Some(url(big)));
        cache.put(cache.key(b"e"), Some(url(big)));
        cache.put(cache.key(b"f"), Some(url(big)));
        // Over 64 MiB: the least recently used (b) went first.
        assert!(cache.bytes <= CACHE_BYTES);
        assert!(cache.get(&b).is_none());
        assert!(cache.get(&a).is_some());
        // Too large for the cache: not kept.
        let huge = cache.key(b"huge");
        cache.put(huge, Some(url(CACHE_BYTES / 2)));
        assert!(cache.get(&huge).is_none());
        for n in 0..CACHE_ENTRIES + 10 {
            cache.put(cache.key(&n.to_le_bytes()), None);
        }
        assert_eq!(cache.entries.len(), CACHE_ENTRIES);
        assert_eq!(
            cache.bytes,
            cache.entries.values().map(Entry::size).sum::<usize>()
        );
    }

    #[tokio::test]
    async fn without_a_helper_images_are_re_encoded_here_once() {
        let images = Images::new(None);
        let gif: ImageBytes = Arc::from(
            &b"GIF89a\x01\x00\x01\x00\x80\x00\x00\x00\x00\x00\xff\xff\xff!\xf9\x04\x01\x00\x00\x00\x00,\x00\x00\x00\x00\x01\x00\x01\x00\x00\x02\x01D\x00;"[..],
        );
        let broken: ImageBytes = Arc::from(&b"GIF89a broken"[..]);
        let first = images
            .reencode(vec![gif.clone(), broken.clone(), gif.clone()])
            .await;
        assert_eq!(first.len(), 2);
        assert_eq!(images.batches(), 1);
        let again = images.reencode(vec![broken.clone(), gif.clone()]).await;
        assert_eq!(again, first, "from the cache");
        assert_eq!(images.batches(), 1, "no second batch");
        assert_eq!(images.known([&gif, &broken]), first);
    }

    #[tokio::test]
    async fn a_helper_that_cannot_start_refuses_without_remembering() {
        let images = Images::new(Some(PathBuf::from("/nonexistent/mail-mcp")));
        let png: ImageBytes = Arc::from(&b"\x89PNG\r\n\x1a\nnot really"[..]);
        let result = images.reencode(vec![png.clone()]).await;
        assert_eq!(result.len(), 1);
        assert!(images.known([&png]).is_empty(), "tried again next time");
        assert_eq!(images.batches(), 1);
    }
}
