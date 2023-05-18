use std::{
    collections::BTreeMap,
    ffi::OsStr,
    fmt::Display,
    hash::Hash,
    io::{self, ErrorKind},
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    },
    time::Instant,
};

use image::io::Reader as ImageReader;
use image::ImageFormat;
use tokio::sync::Mutex;
use tokio::task::JoinHandle;
use warp::{constellation::file::FileType, error::Error, logging::tracing::log};

use crate::utils::ExtensionType;

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
    task: Arc<Mutex<BTreeMap<ThumbnailId, JoinHandle<Result<(ExtensionType, Vec<u8>), Error>>>>>,
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
                .map(ExtensionType::from)
                .unwrap_or(ExtensionType::Other);

            let result = match extension.try_into() {
                Ok(FileType::Mime(media)) => match media.ty().as_str() {
                    "image" => tokio::task::spawn_blocking(move || {
                        let format: ImageFormat = extension.try_into()?;
                        let image = image::open(own_path).map_err(anyhow::Error::from)?;
                        let thumbnail = image.thumbnail(width, height);
                        
                        let mut t_buffer = std::io::Cursor::new(vec![]);
                        let output_format = match format {
                            ImageFormat::WebP if cfg!(not(feature = "webp")) => ImageFormat::Jpeg,
                            _ => format 
                        };

                        thumbnail
                            .write_to(&mut t_buffer, output_format)
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

    pub async fn insert_buffer<S: AsRef<str>>(
        &self,
        name: S,
        buffer: &[u8],
        width: u32,
        height: u32,
    ) -> Result<ThumbnailId, Error> {
        let name = PathBuf::from(name.as_ref());

        let buffer = std::io::Cursor::new(buffer.to_vec());

        let id = ThumbnailId::default();

        let task = tokio::spawn(async move {
            let instance = Instant::now();

            let extension = name
                .extension()
                .and_then(OsStr::to_str)
                .map(ExtensionType::from)
                .unwrap_or(ExtensionType::Other);

            let result = match extension.try_into() {
                Ok(FileType::Mime(media)) => match media.ty().as_str() {
                    "image" => tokio::task::spawn_blocking(move || {
                        let format: ImageFormat = extension.try_into()?;
                        let image = ImageReader::new(buffer)
                            .with_guessed_format()?
                            .decode()
                            .map_err(anyhow::Error::from)?;

                        let thumbnail = image.thumbnail(width, height);
                        let mut t_buffer = std::io::Cursor::new(vec![]);
                        let output_format = match format {
                            ImageFormat::WebP if cfg!(not(feature = "webp")) => ImageFormat::Jpeg,
                            _ => format 
                        };
                        thumbnail
                            .write_to(&mut t_buffer, output_format)
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

    pub async fn get(&self, id: ThumbnailId) -> Result<(ExtensionType, Vec<u8>), Error> {
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
