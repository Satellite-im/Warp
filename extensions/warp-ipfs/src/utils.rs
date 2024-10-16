use bytes::{BufMut, Bytes, BytesMut};
use futures::future::BoxFuture;
use futures::stream::BoxStream;
use futures::{AsyncRead, FutureExt, Stream, StreamExt};
use image::ImageFormat;
use mediatype::MediaTypeBuf;
use std::future::{Future, IntoFuture};
use std::pin::Pin;
use std::str::FromStr;
use std::task::{Context, Poll};
use warp::{
    constellation::{file::FileType, item::FormatType},
    error::Error,
};

#[allow(clippy::upper_case_acronyms)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, derive_more::Display)]
pub enum ExtensionType {
    #[display(fmt = "image/png")]
    PNG,
    #[display(fmt = "image/jpeg")]
    JPG,
    #[display(fmt = "image/svg+xml")]
    SVG,
    #[display(fmt = "image/gif")]
    GIF,
    #[display(fmt = "image/webp")]
    WEBP,
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

impl From<&str> for ExtensionType {
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
            "webp" => Self::WEBP,
            "webm" => Self::WEBM,
            "mkv" => Self::MKV,
            "pdf" => Self::PDF,
            "txt" => Self::TXT,
            "docx" => Self::DOCX,
            "doc" => Self::DOC,
            _ => Self::Other,
        }
    }
}

impl TryFrom<ExtensionType> for ImageFormat {
    type Error = Error;
    fn try_from(value: ExtensionType) -> Result<Self, Self::Error> {
        match value {
            ExtensionType::JPG => Ok(ImageFormat::Jpeg),
            ExtensionType::PNG => Ok(ImageFormat::Png),
            ExtensionType::GIF => Ok(ImageFormat::Gif),
            ExtensionType::ICO => Ok(ImageFormat::Ico),
            ExtensionType::BMP => Ok(ImageFormat::Bmp),
            ExtensionType::WEBP => Ok(ImageFormat::WebP),
            _ => Err(Error::Unimplemented),
        }
    }
}

impl TryFrom<ImageFormat> for ExtensionType {
    type Error = Error;
    fn try_from(value: ImageFormat) -> Result<Self, Self::Error> {
        match value {
            ImageFormat::Jpeg => Ok(ExtensionType::JPG),
            ImageFormat::Png => Ok(ExtensionType::PNG),
            ImageFormat::Gif => Ok(ExtensionType::GIF),
            ImageFormat::Ico => Ok(ExtensionType::ICO),
            ImageFormat::Bmp => Ok(ExtensionType::BMP),
            ImageFormat::WebP => Ok(ExtensionType::WEBP),
            _ => Err(Error::Unimplemented),
        }
    }
}

impl TryFrom<ExtensionType> for MediaTypeBuf {
    type Error = Error;
    fn try_from(ext: ExtensionType) -> Result<Self, Self::Error> {
        let ty = ext.to_string();
        let media = MediaTypeBuf::from_str(&ty).map_err(anyhow::Error::from)?;
        Ok(media)
    }
}

impl From<ExtensionType> for FileType {
    fn from(ext: ExtensionType) -> Self {
        match ext.try_into() {
            Ok(media) => FileType::Mime(media),
            Err(_) => FileType::Generic,
        }
    }
}

impl From<ExtensionType> for FormatType {
    fn from(ext: ExtensionType) -> Self {
        match ext.try_into() {
            Ok(media) => Self::Mime(media),
            Err(_) => Self::Generic,
        }
    }
}

pub struct ByteCollection {
    stream: Option<BoxStream<'static, std::io::Result<Bytes>>>,
    max_size: Option<usize>,
    buffer: BytesMut,
}

impl ByteCollection {
    pub fn new<S>(stream: S) -> Self
    where
        S: Stream<Item = std::io::Result<Bytes>> + Unpin + Send + 'static,
    {
        Self::new_with_max_capacity(stream, 0)
    }

    pub fn new_with_max_capacity<S>(stream: S, capacity: usize) -> Self
    where
        S: Stream<Item = std::io::Result<Bytes>> + Unpin + Send + 'static,
    {
        let stream = stream.boxed();
        Self {
            stream: Some(stream),
            max_size: (capacity > 0).then_some(capacity),
            buffer: BytesMut::new(),
        }
    }

    #[allow(dead_code)]
    pub fn from_bytes<B: Into<Bytes>>(bytes: B) -> Self {
        let bytes = bytes.into();
        let stream = futures::stream::iter(vec![Ok(bytes)]);
        Self::new(stream)
    }

    #[allow(dead_code)]
    pub fn from_bytes_with_capacity<B: Into<Bytes>>(bytes: B, capacity: usize) -> Self {
        let bytes = bytes.into();
        let stream = futures::stream::iter(vec![Ok(bytes)]);
        Self::new_with_max_capacity(stream, capacity)
    }
}

impl Future for ByteCollection {
    type Output = std::io::Result<Bytes>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = &mut *self;

        let Some(st) = this.stream.as_mut() else {
            return Poll::Ready(Err(std::io::ErrorKind::BrokenPipe.into()));
        };

        loop {
            match futures::ready!(st.poll_next_unpin(cx)) {
                Some(Ok(bytes)) => {
                    this.buffer.put(bytes);
                    if let Some(max_size) = this.max_size {
                        if this.buffer.len() > max_size {
                            this.buffer.clear();
                            this.stream.take();
                            return Poll::Ready(Err(std::io::ErrorKind::BrokenPipe.into()));
                        }
                    }
                }
                Some(Err(e)) => {
                    this.buffer.clear();
                    this.stream.take();
                    return Poll::Ready(Err(e));
                }
                None => {
                    this.stream.take();
                    let bytes_mut = this.buffer.split();
                    let bytes = bytes_mut.freeze();
                    return Poll::Ready(Ok(bytes));
                }
            }
        }
    }
}

// Small utility that converts AsyncRead to a Stream, while supporting max size from that stream
pub struct ReaderStream<R> {
    reader: Option<R>,
    buffer: usize,
    size: usize,
    max_cap: Option<usize>,
}

impl<R> ReaderStream<R>
where
    R: AsyncRead + Unpin + Send + 'static,
{
    #[allow(dead_code)]
    pub fn from_reader(reader: R) -> Self {
        Self::from_reader_with_cap(reader, 512, None)
    }

    pub fn from_reader_with_cap(reader: R, buffer: usize, max_cap: Option<usize>) -> Self {
        Self {
            reader: Some(reader),
            buffer,
            size: 0,
            max_cap,
        }
    }
}

impl<R> Stream for ReaderStream<R>
where
    R: AsyncRead + Unpin + Send + 'static,
{
    type Item = std::io::Result<Bytes>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let this = &mut *self;

        let Some(reader) = this.reader.as_mut() else {
            return Poll::Ready(None);
        };

        let buf_size: usize = this.buffer;
        let mut buffer = vec![0u8; buf_size];

        match futures::ready!(Pin::new(reader).poll_read(cx, &mut buffer)) {
            Ok(0) => {
                this.reader.take();
                if this.size == 0 {
                    return Poll::Ready(Some(Err(std::io::ErrorKind::BrokenPipe.into())));
                }
                Poll::Ready(None)
            }
            Ok(size) => {
                this.size += size;
                if let Some(max_size) = this.max_cap {
                    if size > max_size {
                        this.reader.take();
                        return Poll::Ready(Some(Err(std::io::Error::new(
                            std::io::ErrorKind::Other,
                            "max size has been reached",
                        ))));
                    }
                }
                let new_buf = &buffer[..size];
                let buf = Bytes::copy_from_slice(new_buf);
                Poll::Ready(Some(Ok(buf)))
            }
            Err(e) => {
                this.reader.take();
                Poll::Ready(Some(Err(e)))
            }
        }
    }
}

impl<R> IntoFuture for ReaderStream<R>
where
    R: AsyncRead + Unpin + Send + 'static,
{
    type Output = std::io::Result<Bytes>;
    type IntoFuture = BoxFuture<'static, Self::Output>;

    fn into_future(self) -> Self::IntoFuture {
        ByteCollection::new(self).boxed()
    }
}

// #[derive(Default)]
// pub struct ReplaceableFuture<F> {
//     fut: Option<F>,
//     waker: Option<Waker>,
// }

// impl<F: Future + Unpin> ReplaceableFuture<F> {
//     pub fn new(fut: F) -> Self {
//         Self {
//             fut: Some(fut),
//             waker: None,
//         }
//     }

//     pub fn replace(&mut self, fut: F) -> Option<F> {
//         let old_fut = self.fut.replace(fut);
//         if let Some(waker) = self.waker.take() {
//             waker.wake();
//         }
//         old_fut
//     }
// }

// impl<F: Future + Unpin> Stream for ReplaceableFuture<F> {
//     type Item = F::Output;
//     fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
//         let Some(fut) = self.fut.as_mut() else {
//             self.waker.replace(cx.waker().clone());
//             return Poll::Pending;
//         };

//         match Pin::new(fut).poll(cx) {
//             Poll::Ready(value) => {
//                 self.fut.take();
//                 Poll::Ready(Some(value))
//             }
//             Poll::Pending => {
//                 self.waker.replace(cx.waker().clone());
//                 Poll::Pending
//             }
//         }
//     }
// }

#[cfg(test)]
mod test {
    use crate::utils::{ByteCollection, ReaderStream};
    use bytes::Bytes;

    #[tokio::test]
    async fn async_read_to_stream() -> std::io::Result<()> {
        let data = Bytes::copy_from_slice(b"hello, world");

        let cursor = futures::io::Cursor::new(data.clone());

        let st = ReaderStream::from_reader(cursor);

        let bytes = st.await?;

        assert_eq!(bytes, data);

        Ok(())
    }

    #[tokio::test]
    async fn byte_collection_from_stream() -> std::io::Result<()> {
        let data = Bytes::copy_from_slice(b"hello, world");

        let st = futures::stream::iter(vec![Ok(data.clone())]);

        let col = ByteCollection::new(st);

        let bytes = col.await?;

        assert_eq!(bytes, data);

        Ok(())
    }

    #[tokio::test]
    async fn async_read_with_max_size() -> std::io::Result<()> {
        let data = Bytes::copy_from_slice(b"hello, world");

        let cursor = futures::io::Cursor::new(data.clone());

        let st = ReaderStream::from_reader_with_cap(cursor, 512, Some(data.len()));

        let bytes = st.await?;

        assert_eq!(bytes, data);

        Ok(())
    }

    #[tokio::test]
    async fn cannot_read_due_to_max_size() -> std::io::Result<()> {
        let data = b"hello, world".to_vec();

        let cursor = futures::io::Cursor::new(data.to_vec());

        let st = ReaderStream::from_reader_with_cap(cursor, 512, Some(11));

        assert!(st.await.is_err());

        Ok(())
    }
}
