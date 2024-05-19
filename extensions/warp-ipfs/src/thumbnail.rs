use image::io::Reader as ImageReader;
use image::ImageFormat;
use rust_ipfs::{Ipfs, IpfsPath};
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
};
use tokio::sync::Mutex;
use tokio::task::JoinHandle;
use warp::{constellation::file::FileType, error::Error};
use web_time::Instant;

use crate::{store::document::image_dag::ImageDag, utils::ExtensionType};

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

type TaskMap = BTreeMap<ThumbnailId, JoinHandle<Result<(ExtensionType, IpfsPath, Vec<u8>), Error>>>;

#[derive(Clone)]
pub struct ThumbnailGenerator {
    ipfs: Ipfs,
    tasks: Arc<Mutex<TaskMap>>,
}

impl ThumbnailGenerator {
    pub fn new(ipfs: Ipfs) -> Self {
        Self {
            ipfs,
            tasks: Arc::default(),
        }
    }

    pub async fn insert<P: AsRef<Path>>(
        &self,
        path: P,
        width: u32,
        height: u32,
        output_exact: bool,
    ) -> Result<ThumbnailId, Error> {
        let path = path.as_ref();
        if !path.is_file() {
            return Err(io::Error::from(ErrorKind::NotFound).into());
        }

        let own_path = path.to_path_buf();
        let id = ThumbnailId::default();
        let ipfs = self.ipfs.clone();

        let task = tokio::spawn(async move {
            let instance = Instant::now();
            //TODO: Read file header to determine real file type for anything like images, videos and documents.
            let extension = own_path
                .extension()
                .and_then(OsStr::to_str)
                .map(ExtensionType::from)
                .unwrap_or(ExtensionType::Other);

            let result = match extension.into() {
                FileType::Mime(media) => match media.ty().as_str() {
                    "image" => tokio::task::spawn_blocking(move || {
                        let format: ImageFormat = extension.try_into()?;
                        let image = image::open(own_path).map_err(anyhow::Error::from)?;
                        let width = width.min(image.width());
                        let height = height.min(image.height());

                        let thumbnail = image.thumbnail(width, height);

                        let mut t_buffer = std::io::Cursor::new(vec![]);
                        let output_format = match (output_exact, format) {
                            (false, _) => ImageFormat::Jpeg,
                            (true, ImageFormat::WebP) if cfg!(not(feature = "webp")) => {
                                ImageFormat::Jpeg
                            }
                            (true, format) => format,
                        };

                        thumbnail
                            .write_to(&mut t_buffer, output_format)
                            .map_err(anyhow::Error::from)?;
                        Ok::<_, Error>((
                            ExtensionType::try_from(output_format)?,
                            t_buffer.into_inner(),
                        ))
                    })
                    .await
                    .map_err(anyhow::Error::from)?,
                    _ => Err(Error::Unimplemented),
                },
                _ => Err(Error::Other),
            };

            let stop = instance.elapsed();

            tracing::trace!("Took: {}ms to complete task for {}", stop.as_millis(), id);

            let (ty, data) = result?;

            let size = data.len();

            let path = ipfs.add_unixfs(data.clone()).await?;

            let link = *path.root().cid().expect("valid cid");

            let image_dag = ImageDag {
                link,
                size: size as _,
                mime: ty.into(),
            };

            let cid = ipfs.dag().put().serialize(image_dag).await?;

            Ok((ty, IpfsPath::from(cid), data))
        });

        self.tasks.lock().await.insert(id, task);

        Ok(id)
    }

    pub async fn insert_buffer<S: AsRef<str>>(
        &self,
        name: S,
        buffer: &[u8],
        width: u32,
        height: u32,
        output_exact: bool,
    ) -> ThumbnailId {
        let name = PathBuf::from(name.as_ref());

        let buffer = std::io::Cursor::new(buffer.to_vec());

        let id = ThumbnailId::default();

        let ipfs = self.ipfs.clone();
        let task = tokio::spawn(async move {
            let instance = Instant::now();

            let extension = name
                .extension()
                .and_then(OsStr::to_str)
                .map(ExtensionType::from)
                .unwrap_or(ExtensionType::Other);

            let result = match extension.into() {
                FileType::Mime(media) => match media.ty().as_str() {
                    "image" => tokio::task::spawn_blocking(move || {
                        let format: ImageFormat = extension.try_into()?;
                        let image = ImageReader::new(buffer)
                            .with_guessed_format()?
                            .decode()
                            .map_err(anyhow::Error::from)?;

                        let width = width.min(image.width());
                        let height = height.min(image.height());

                        let thumbnail = image.thumbnail(width, height);
                        let mut t_buffer = std::io::Cursor::new(vec![]);
                        let output_format = match (output_exact, format) {
                            (false, _) => ImageFormat::Jpeg,
                            (true, ImageFormat::WebP) if cfg!(not(feature = "webp")) => {
                                ImageFormat::Jpeg
                            }
                            (true, format) => format,
                        };
                        thumbnail
                            .write_to(&mut t_buffer, output_format)
                            .map_err(anyhow::Error::from)?;
                        Ok::<_, Error>((
                            ExtensionType::try_from(output_format)?,
                            t_buffer.into_inner(),
                        ))
                    })
                    .await
                    .map_err(anyhow::Error::from)?,
                    _ => Err(Error::Unimplemented),
                },
                _ => Err(Error::Other),
            };

            let stop = instance.elapsed();

            tracing::trace!("Took: {}ms to complete task for {}", stop.as_millis(), id);

            let (ty, data) = result?;

            let path = ipfs.add_unixfs(data.clone()).await?;

            let link = *path.root().cid().expect("valid cid");

            let image_dag = ImageDag {
                link,
                size: data.len() as _,
                mime: ty.into(),
            };

            let cid = ipfs.dag().put().serialize(image_dag).await?;

            Ok((ty, IpfsPath::from(cid), data))
        });

        self.tasks.lock().await.insert(id, task);

        id
    }

    #[allow(dead_code)]
    pub async fn cancel(&self, id: ThumbnailId) {
        let task = self.tasks.lock().await.remove(&id);
        if let Some(task) = task {
            task.abort();
        }
    }

    pub async fn get(&self, id: ThumbnailId) -> Result<(ExtensionType, IpfsPath, Vec<u8>), Error> {
        let task = self.tasks.lock().await.remove(&id);
        let task = task.ok_or(Error::Other)?;
        task.await.map_err(anyhow::Error::from)?
    }

    #[allow(dead_code)]
    pub async fn is_finished(&self, id: ThumbnailId) -> Result<bool, Error> {
        if let Some(task) = self.tasks.lock().await.get(&id) {
            return Ok(task.is_finished());
        }
        Err(Error::Other)
    }
}
