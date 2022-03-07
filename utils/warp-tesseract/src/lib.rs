use warp_common::Result;


//TODO: Investigate if this is a viable method of handling an encrypted datastore


pub trait Tesseract {
    fn unlock(&self, passphrase: Vec<u8>) -> Result<&mut Box<dyn TesseractStore>>;
}

pub trait TesseractStore {
    fn set(&mut self, key: &str, value: &str) -> Result<()>;
    fn exist(&self, key: &str) -> bool;
    fn retrieve(&self, key: &str) -> Result<&str>;
    fn delete(&mut self, key: &str) -> Result<()>;
    fn clear(&mut self) -> Result<()>;
}

