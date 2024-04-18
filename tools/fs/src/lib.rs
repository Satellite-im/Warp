use std::{io, path::Path};

#[cfg(target_arch = "wasm32")]
use gloo::storage::{LocalStorage, Storage};

/// Read the contents of the file at path
pub async fn read(path: impl AsRef<Path>) -> io::Result<Vec<u8>> {
    #[cfg(not(target_arch = "wasm32"))]
    return tokio::fs::read(path).await;

    #[cfg(target_arch = "wasm32")]
    {
        let path = path.as_ref().to_str().ok_or(io::ErrorKind::InvalidInput)?;
        match LocalStorage::get(&path) {
            Ok(ok) => Ok(ok),
            Err(e) => Err(io::Error::other(e)),
        }
    }
}

/// Write the contents of the file at path
pub async fn write(path: impl AsRef<Path>, contents: impl AsRef<[u8]>) -> io::Result<()> {
    #[cfg(not(target_arch = "wasm32"))]
    return tokio::fs::write(path, contents).await;

    #[cfg(target_arch = "wasm32")]
    {
        let path = path.as_ref().to_str().ok_or(io::ErrorKind::InvalidInput)?;
        match LocalStorage::set(path, contents.as_ref().to_owned()) {
            Ok(_) => Ok(()),
            Err(e) => Err(io::Error::other(e)),
        }
    }
}

/// Create all directories in path
pub async fn create_dir_all(path: impl AsRef<Path>) -> io::Result<()> {
    #[cfg(not(target_arch = "wasm32"))]
    return tokio::fs::create_dir_all(path).await;

    //Dirs don't need to be created in wasm since we are using the path as a key in LocalStorage
    #[cfg(target_arch = "wasm32")]
    Ok(())
}

/// Delete the file at path
pub async fn remove_file(path: impl AsRef<Path>) -> io::Result<()> {
    #[cfg(not(target_arch = "wasm32"))]
    return tokio::fs::remove_file(path).await;

    #[cfg(target_arch = "wasm32")]
    {
        let path = path.as_ref().to_str().ok_or(io::ErrorKind::InvalidInput)?;
        LocalStorage::delete(path);
        Ok(())
    }
}

/// Get the size of the file at path
pub async fn file_size(path: impl AsRef<Path>) -> io::Result<usize> {
    #[cfg(not(target_arch = "wasm32"))]
    return Ok(tokio::fs::metadata(&path).await?.len() as usize);

    #[cfg(target_arch = "wasm32")]
    Ok(read(path).await?.len())
}

/// A reference to an open file on the filesystem. On wasm, this instead holds a Vec<u8> of the data already read from LocalStorage.
pub struct File {
    #[cfg(not(target_arch = "wasm32"))]
    file: tokio::fs::File,
    #[cfg(target_arch = "wasm32")]
    file: Vec<u8>,
}

impl File {
    /// Attempts to open a file in read-only mode. On wasm, this reads the file's data from LocalStorage.
    pub async fn open(path: impl AsRef<Path>) -> io::Result<File> {
        #[cfg(not(target_arch = "wasm32"))]
        return Ok(Self {
            file: tokio::fs::File::open(&path).await?,
        });

        #[cfg(target_arch = "wasm32")]
        Ok(Self {
            file: read(path).await?,
        })
    }
    /// Returns the size of the file contents
    pub async fn file_size(&self) -> io::Result<usize> {
        #[cfg(not(target_arch = "wasm32"))]
        return Ok(self.file.metadata().await?.len() as usize);

        #[cfg(target_arch = "wasm32")]
        Ok(self.file.len())
    }
    /// Wraps self with a compatibility layer that implements futures_io::AsyncRead.
    pub fn compat(self) -> impl futures_io::AsyncRead {
        #[cfg(not(target_arch = "wasm32"))]
        {
            use tokio_util::compat::TokioAsyncReadCompatExt;
            return self.file.compat();
        }

        #[cfg(target_arch = "wasm32")]
        {
            LocalStorageReader::new(self.file)
        }
    }
}

/// Allows some arbitrary data to be read with AsyncRead. We needed this to be able use data from LocalStorage in place of streaming a file from the filesystem.
struct LocalStorageReader {
    data: Vec<u8>,
    bytes_read: usize,
}
impl LocalStorageReader {
    /// Create a new LocalStorageReader
    fn new(data: Vec<u8>) -> Self {
        Self {
            data,
            bytes_read: 0,
        }
    }
}
impl futures_io::AsyncRead for LocalStorageReader {
    fn poll_read(
        self: std::pin::Pin<&mut Self>,
        _cx: &mut std::task::Context<'_>,
        buf: &mut [u8],
    ) -> std::task::Poll<io::Result<usize>> {
        // calculate how many bytes we can read from self.data to put into the buffer
        let bytes_to_read = std::cmp::min(self.data.len() - self.bytes_read, buf.len());
        // read bytes from self.data and write them into the buffer
        for i in 0..bytes_to_read {
            buf[i] = self.data[self.bytes_read + i]
        }
        // update our tracking of how many bytes we've read from self.data
        self.get_mut().bytes_read += bytes_to_read;
        // return how many bytes were written into the buffer
        core::task::Poll::Ready(Ok(bytes_to_read))
    }
}

#[cfg(test)]
mod test {
    use crate::File;
    use futures_util::AsyncReadExt;

    /// tests that our LocalStorageReader properly reads data.
    #[tokio::test]
    async fn file_compat_reader() -> futures_io::Result<()> {
        let path = "test_file.txt";
        let contents = "hello";

        // Create file, read it and compare result
        crate::write(path, contents).await?;
        let data = crate::read(path).await?;
        assert_eq!(
            data,
            contents.as_bytes(),
            "expected data to be {:?}",
            contents.as_bytes()
        );

        // use the data to stream file contents, then compare the stream result
        let mut reader = crate::LocalStorageReader::new(data.clone());
        let buffer_size = 2;
        let mut buffer = vec![0u8; buffer_size];
        let mut stream_result = vec![];
        loop {
            match reader.read(&mut buffer).await {
                Ok(n) => {
                    stream_result.append(&mut buffer.clone()[0..n].to_vec());
                    if n < buffer_size {
                        break;
                    }
                }
                Err(e) => return Err(e),
            }
        }
        assert_eq!(
            stream_result, data,
            "expected stream_result to be {:?}",
            data
        );

        // Cleanup
        crate::remove_file(path).await?;

        Ok(())
    }
}
