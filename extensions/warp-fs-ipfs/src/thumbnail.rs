use std::{
    collections::BTreeMap,
    ffi::OsStr,
    hash::Hash,
    io::{self, ErrorKind},
    path::Path,
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    },
    time::Instant, fmt::Display,
};

use tokio::sync::Mutex;
use tokio::task::JoinHandle;
use warp::{constellation::file::FileType, error::Error, logging::tracing::log};

static GLOBAL_ID: AtomicUsize = AtomicUsize::new(0);

#[derive(Debug, Copy, Clone, Hash, PartialEq, Eq, PartialOrd, Ord)]
pub struct ThumbnailId(usize);

impl Display for ThumbnailId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl core::ops::Deref for ThumbnailId {
    type Target = usize;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl Default for ThumbnailId {
    fn default() -> Self {
        ThumbnailId(GLOBAL_ID.fetch_add(1, Ordering::SeqCst))
    }
}

#[derive(Default, Clone)]
#[allow(clippy::type_complexity)]
pub struct ThumbnailGenerator {
    task: Arc<
        Mutex<BTreeMap<ThumbnailId, JoinHandle<Result<(ThumbnailExtensionType, Vec<u8>), Error>>>>,
    >,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ThumbnailExtensionType {
    PNG,
    JPG,
    SVG,
    GIF,
    BMP,
    ICO,
    MP4,
    AVI,
    WEBM,
    MKV,
    PDF,
    DOC,
    DOCX,
    TXT,
    Invalid,
}

impl From<&str> for ThumbnailExtensionType {
    fn from(ext: &str) -> Self {
        match ext.to_lowercase().as_str() {
            "png" => Self::PNG,
            "jpeg" | "jpg" => Self::JPG,
            "svg" => Self::SVG,
            "gif" => Self::GIF,
            "bmp" => Self::BMP,
            "ico" => Self::ICO,
            "mp4" => Self::MP4,
            "avi" => Self::AVI,
            "webm" => Self::WEBM,
            "mkv" => Self::MKV,
            "pdf" => Self::PDF,
            _ => Self::Invalid,
        }
    }
}

impl From<ThumbnailExtensionType> for FileType {
    fn from(ext: ThumbnailExtensionType) -> Self {
        match ext {
            ThumbnailExtensionType::PNG
            | ThumbnailExtensionType::JPG
            | ThumbnailExtensionType::SVG
            | ThumbnailExtensionType::BMP
            | ThumbnailExtensionType::GIF
            | ThumbnailExtensionType::ICO => FileType::Image,
            _ => FileType::Other,
        }
    }
}

impl ThumbnailGenerator {
    pub async fn insert<P: AsRef<Path>>(&self, path: P) -> Result<ThumbnailId, Error> {
        let path = path.as_ref();
        if !path.is_file() {
            return Err(io::Error::from(ErrorKind::NotFound).into());
        }

        let own_path = path.to_path_buf();
        let id = ThumbnailId::default();

        let task = tokio::spawn(async move {
            let instance = Instant::now();
            //TODO: Read file header to determine real file type for anything like images, videos and documents.
            let extension = own_path
                .extension()
                .and_then(OsStr::to_str)
                .map(ThumbnailExtensionType::from)
                .unwrap_or(ThumbnailExtensionType::Invalid);

            let result = if matches!(extension.into(), FileType::Image) {
                tokio::task::spawn_blocking(move || {
                    let image = image::open(own_path).map_err(anyhow::Error::from)?;
                    let thumbnail = image.thumbnail(500, 500);
                    let bytes = thumbnail.as_bytes();
                    Ok::<_, Error>((extension, bytes.to_vec()))
                })
                .await
                .map_err(anyhow::Error::from)?
            } else {
                Err(Error::Other)
            };

            let stop = instance.elapsed();

            log::trace!("Took: {}ms to complete task for {}", stop.as_millis(), id);
            result
        });

        self.task.lock().await.insert(id, task);

        Ok(id)
    }

    pub async fn cancel(&self, id: ThumbnailId) {
        let task = self.task.lock().await.remove(&id);
        if let Some(task) = task {
            task.abort();
        }
    }

    pub async fn get(&self, id: ThumbnailId) -> Result<(ThumbnailExtensionType, Vec<u8>), Error> {
        let task = self.task.lock().await.remove(&id);
        let task = task.ok_or(Error::Other)?;
        task.await.map_err(anyhow::Error::from)?
    }

    pub async fn is_finished(&self, id: ThumbnailId) -> Result<bool, Error> {
        if let Some(task) = self.task.lock().await.get(&id) {
            return Ok(task.is_finished());
        }
        Err(Error::Other)
    }
}
