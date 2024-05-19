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
    {
        _ = path;
        Ok(())
    }
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
