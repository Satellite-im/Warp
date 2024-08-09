use bytes::Bytes;
use futures::channel::oneshot;
use image::{ImageFormat, ImageReader};
use rust_ipfs::{Ipfs, IpfsPath};
use std::{
    collections::BTreeMap,
    ffi::OsStr,
    fmt::Display,
    hash::Hash,
    path::PathBuf,
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    },
};

use futures::Stream;
use std::io::{self, Cursor};

#[allow(unused_imports)]
use std::{io::ErrorKind, path::Path};

use tokio::sync::Mutex;
use warp::{constellation::file::FileType, error::Error};
use web_time::Instant;

use crate::utils::ByteCollection;
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

type TaskMap = BTreeMap<
    ThumbnailId,
    futures::channel::oneshot::Receiver<Result<(ExtensionType, IpfsPath, Bytes), Error>>,
>;

#[derive(Clone)]
pub struct ThumbnailGenerator {
    ipfs: Ipfs,
    tasks: Arc<Mutex<TaskMap>>,
}

impl ThumbnailGenerator {
    pub fn new(ipfs: &Ipfs) -> Self {
        Self {
            ipfs: ipfs.clone(),
            tasks: Arc::default(),
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
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

        let id = ThumbnailId::default();
        let own_path = path.to_path_buf();

        let ipfs = self.ipfs.clone();

        let (tx, rx) = oneshot::channel();
        crate::rt::spawn(async move {
            let res = async move {
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

                            let mut t_buffer = Cursor::new(vec![]);
                            let output_format = match (output_exact, format) {
                                (false, _) => ImageFormat::Jpeg,
                                (true, format) => format,
                            };

                            thumbnail
                                .write_to(&mut t_buffer, output_format)
                                .map_err(anyhow::Error::from)?;
                            Ok::<_, Error>((
                                ExtensionType::try_from(output_format)?,
                                Bytes::from(t_buffer.into_inner()),
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

                let cid = ipfs.put_dag(image_dag).await?;
                Ok((ty, IpfsPath::from(cid), data))
            };

            _ = tx.send(res.await)
        });

        self.tasks.lock().await.insert(id, rx);

        Ok(id)
    }

    pub async fn insert_stream<
        N: AsRef<str>,
        S: Stream<Item = io::Result<Bytes>> + Unpin + Send + 'static,
    >(
        &self,
        name: N,
        stream: S,
        width: u32,
        height: u32,
        output_exact: bool,
        max_size: usize,
    ) -> ThumbnailId {
        let name = PathBuf::from(name.as_ref());

        // Note: We have a max of 20mb for the thumbnail generation from stream so if a file exceeds this capacity
        // the thumbnail will not be generated.
        // TODO: We could probably check the signature of the stream first before deciding what to do with it. If its an invalid
        //       stream we could error out and prevent any attempts of generating the thumbnail.

        let bytes = ByteCollection::new_with_max_capacity(stream, max_size);

        let id = ThumbnailId::default();

        let ipfs = self.ipfs.clone();

        let (tx, rx) = oneshot::channel();
        crate::rt::spawn(async move {
            let res = async move {
                let instant = Instant::now();

                let extension = name
                    .extension()
                    .and_then(OsStr::to_str)
                    .map(ExtensionType::from)
                    .unwrap_or(ExtensionType::Other);

                let result = match extension.into() {
                    FileType::Mime(media) => match media.ty().as_str() {
                        "image" => {
                            let format: ImageFormat = extension.try_into()?;

                            let data = bytes.await.map(|b| b.to_vec())?;
                            let cursor = Cursor::new(data);
                            let image = ImageReader::new(cursor)
                                .with_guessed_format()?
                                .decode()
                                .map_err(anyhow::Error::from)?;

                            let width = width.min(image.width());
                            let height = height.min(image.height());

                            let thumbnail = image.thumbnail(width, height);
                            let mut t_buffer = std::io::Cursor::new(Vec::new());
                            let output_format = match (output_exact, format) {
                                (false, _) => ImageFormat::Jpeg,
                                (true, format) => format,
                            };
                            thumbnail
                                .write_to(&mut t_buffer, output_format)
                                .map_err(anyhow::Error::from)?;
                            Ok::<_, Error>((
                                ExtensionType::try_from(output_format)?,
                                Bytes::from(t_buffer.into_inner()),
                            ))
                        }
                        _ => Err(Error::Unimplemented),
                    },
                    _ => Err(Error::Other),
                };

                let stop = instant.elapsed();

                let (ty, data) = result?;

                tracing::trace!("Took: {}ms to complete for {}", stop.as_millis(), id);

                let path = ipfs.add_unixfs(data.clone()).await?;

                let link = *path.root().cid().expect("valid cid");

                let image_dag = ImageDag {
                    link,
                    size: data.len() as _,
                    mime: ty.into(),
                };

                let cid = ipfs.dag().put().serialize(image_dag).await?;

                Ok((ty, IpfsPath::from(cid), data))
            };
            _ = tx.send(res.await);
        });

        self.tasks.lock().await.insert(id, rx);

        id
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

        let (tx, rx) = oneshot::channel();
        crate::rt::spawn(async move {
            let res = async move {
                let instance = Instant::now();

                let extension = name
                    .extension()
                    .and_then(OsStr::to_str)
                    .map(ExtensionType::from)
                    .unwrap_or(ExtensionType::Other);

                let result = match extension.into() {
                    FileType::Mime(media) => match media.ty().as_str() {
                        "image" => {
                            let format: ImageFormat = extension.try_into()?;
                            let image = ImageReader::new(buffer)
                                .with_guessed_format()?
                                .decode()
                                .map_err(anyhow::Error::from)?;

                            let width = width.min(image.width());
                            let height = height.min(image.height());

                            let thumbnail = image.thumbnail(width, height);
                            let mut t_buffer = Cursor::new(vec![]);
                            let output_format = match (output_exact, format) {
                                (false, _) => ImageFormat::Jpeg,
                                (true, format) => format,
                            };
                            thumbnail
                                .write_to(&mut t_buffer, output_format)
                                .map_err(anyhow::Error::from)?;
                            Ok::<_, Error>((
                                ExtensionType::try_from(output_format)?,
                                Bytes::from(t_buffer.into_inner()),
                            ))
                        }
                        _ => Err(Error::Unimplemented),
                    },
                    _ => Err(Error::Other),
                };

                let stop = instance.elapsed();

                let (ty, data) = result?;

                tracing::trace!("Took: {}ms to complete for {}", stop.as_millis(), id);

                let path = ipfs.add_unixfs(data.clone()).await?;

                let link = *path.root().cid().expect("valid cid");

                let image_dag = ImageDag {
                    link,
                    size: data.len() as _,
                    mime: ty.into(),
                };

                let cid = ipfs.dag().put().serialize(image_dag).await?;

                Ok((ty, IpfsPath::from(cid), data))
            };
            _ = tx.send(res.await);
        });

        self.tasks.lock().await.insert(id, rx);

        id
    }

    pub async fn get(&self, id: ThumbnailId) -> Result<(ExtensionType, IpfsPath, Bytes), Error> {
        let task = self.tasks.lock().await.remove(&id);
        let task = task.ok_or(Error::Other)?;
        task.await.map_err(anyhow::Error::from)?
    }
}
