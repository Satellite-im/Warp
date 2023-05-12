use std::{
    collections::BTreeMap,
    ffi::OsStr,
    fmt::Display,
    hash::Hash,
    io::{self, ErrorKind},
    path::Path,
    str::FromStr,
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    },
    time::Instant,
};

use image::ImageFormat;
use mediatype::MediaTypeBuf;
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

#[derive(Clone, Copy, PartialEq, Eq, derive_more::Display)]
pub enum ThumbnailExtensionType {
    #[display(fmt = "image/png")]
    PNG,
    #[display(fmt = "image/jpeg")]
    JPG,
    #[display(fmt = "image/svg+xml")]
    SVG,
    #[display(fmt = "image/gif")]
    GIF,
    #[display(fmt = "image/bmp")]
    BMP,
    #[display(fmt = "image/vnd.microsoft.icon")]
    ICO,
    #[display(fmt = "video/mp4")]
    MP4,
    #[display(fmt = "video/x-msvideo")]
    AVI,
    #[display(fmt = "video/webm")]
    WEBM,
    #[display(fmt = "video/x-matroska")]
    MKV,
    #[display(fmt = "application/pdf")]
    PDF,
    #[display(fmt = "application/msword")]
    DOC,
    #[display(fmt = "application/vnd.openxmlformats-officedocument.wordprocessingml.document")]
    DOCX,
    #[display(fmt = "text/plain")]
    TXT,
    #[display(fmt = "application/octet-stream")]
    Other,
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
            _ => Self::Other,
        }
    }
}

impl TryFrom<ThumbnailExtensionType> for ImageFormat {
    type Error = Error;
    fn try_from(value: ThumbnailExtensionType) -> Result<Self, Self::Error> {
        match value {
            ThumbnailExtensionType::JPG => Ok(ImageFormat::Jpeg),
            ThumbnailExtensionType::PNG => Ok(ImageFormat::Png),
            ThumbnailExtensionType::GIF => Ok(ImageFormat::Gif),
            ThumbnailExtensionType::ICO => Ok(ImageFormat::Ico),
            ThumbnailExtensionType::BMP => Ok(ImageFormat::Bmp),
            _ => Err(Error::Unimplemented),
        }
    }
}

impl TryFrom<ThumbnailExtensionType> for MediaTypeBuf {
    type Error = Error;
    fn try_from(ext: ThumbnailExtensionType) -> Result<Self, Self::Error> {
        let ty = ext.to_string();
        let media = MediaTypeBuf::from_str(&ty).map_err(anyhow::Error::from)?;
        Ok(media)
    }
}

impl TryFrom<ThumbnailExtensionType> for FileType {
    type Error = Error;
    fn try_from(ext: ThumbnailExtensionType) -> Result<Self, Self::Error> {
        match ext.try_into() {
            Ok(media) => Ok(FileType::Mime(media)),
            Err(_) => Ok(FileType::Generic),
        }
    }
}

impl ThumbnailGenerator {
    pub async fn insert<P: AsRef<Path>>(
        &self,
        path: P,
        width: u32,
        height: u32,
    ) -> Result<ThumbnailId, Error> {
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
                .unwrap_or(ThumbnailExtensionType::Other);

            let result = match extension.try_into() {
                Ok(FileType::Mime(media)) => match media.ty().as_str() {
                    "image" => tokio::task::spawn_blocking(move || {
                        let format: ImageFormat = extension.try_into()?;
                        let image = image::open(own_path).map_err(anyhow::Error::from)?;
                        let thumbnail = image.thumbnail(width, height);
                        let mut t_buffer = std::io::Cursor::new(vec![]);
                        thumbnail
                            .write_to(&mut t_buffer, format)
                            .map_err(anyhow::Error::from)?;
                        Ok::<_, Error>((extension, t_buffer.into_inner()))
                    })
                    .await
                    .map_err(anyhow::Error::from)?,
                    _ => Err(Error::Unimplemented),
                },
                _ => Err(Error::Other),
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
