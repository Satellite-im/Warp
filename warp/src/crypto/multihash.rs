//TODO: Possibly reduce and simplify
#![allow(clippy::result_large_err)]
use multihash::{
    derive::Multihash, Blake2b256, Blake2b512, Blake2s128, Blake2s256, Blake3_256, Hasher,
    Identity256, Keccak224, Keccak256, Keccak384, Keccak512, MultihashDigest, Sha1, Sha2_256,
    Sha2_512, Sha3_224, Sha3_256, Sha3_384, Sha3_512,
};

#[derive(Clone, Copy, Debug, Eq, Multihash, PartialEq)]
#[mh(alloc_size = 64)]
pub enum Code {
    #[mh(code = 0x00, hasher = Identity256)]
    Identity,
    #[mh(code = 0x11, hasher = Sha1)]
    Sha1,
    #[mh(code = 0x12, hasher = Sha2_256)]
    Sha2_256,
    #[mh(code = 0x13, hasher = Sha2_512)]
    Sha2_512,
    #[mh(code = 0x17, hasher = Sha3_224)]
    Sha3_224,
    #[mh(code = 0x16, hasher = Sha3_256)]
    Sha3_256,
    #[mh(code = 0x15, hasher = Sha3_384)]
    Sha3_384,
    #[mh(code = 0x14, hasher = Sha3_512)]
    Sha3_512,
    #[mh(code = 0x1a, hasher = Keccak224)]
    Keccak224,
    #[mh(code = 0x1b, hasher = Keccak256)]
    Keccak256,
    #[mh(code = 0x1c, hasher = Keccak384)]
    Keccak384,
    #[mh(code = 0x1d, hasher = Keccak512)]
    Keccak512,
    #[mh(code = 0xb220, hasher = Blake2b256)]
    Blake2b256,
    #[mh(code = 0xb240, hasher = Blake2b512)]
    Blake2b512,
    #[mh(code = 0xb250, hasher = Blake2s128)]
    Blake2s128,
    #[mh(code = 0xb260, hasher = Blake2s256)]
    Blake2s256,
    #[mh(code = 0x1e, hasher = Blake3_256)]
    Blake3_256,
}

macro_rules! create_hash_functions {
    ($code:expr) => {
        paste::item! {
            pub fn [<$code:lower _multihash_slice>](slice: &[u8]) -> Result<Vec<u8>, crate::error::Error> {
                multihash_slice::<$code>(Code::$code, slice).map_err(crate::error::Error::from)
            }

            pub fn [<$code:lower _multihash_file>]<P: AsRef<std::path::Path>>(file: P) -> Result<Vec<u8>, crate::error::Error> {
                multihash_file::<$code, _>(Code::$code, file).map_err(crate::error::Error::from)
            }
        }
    };
}

pub fn multihash_slice<H: Hasher + Default>(code: Code, slice: &[u8]) -> anyhow::Result<Vec<u8>> {
    let mut hasher = H::default();
    hasher.update(slice);
    let digest = code.wrap(hasher.finalize())?;
    Ok(digest.to_bytes())
}

pub fn multihash_reader<H: Hasher + Default + std::io::Write>(
    code: Code,
    reader: &mut impl std::io::Read,
) -> anyhow::Result<Vec<u8>> {
    let mut hasher = H::default();
    std::io::copy(reader, &mut hasher)?;
    let digest = code.wrap(hasher.finalize())?;
    Ok(digest.to_bytes())
}

pub fn multihash_file<H: Hasher + Default, P: AsRef<std::path::Path>>(
    code: Code,
    file: P,
) -> anyhow::Result<Vec<u8>> {
    use std::io::Read;
    let mut hasher = H::default();
    let mut reader = std::fs::File::open(file)?;
    loop {
        let mut buffer = [0u8; 512];
        match reader.read(&mut buffer) {
            Ok(0) => break,
            Ok(size) => hasher.update(&buffer[..size]),
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(e) => return Err(anyhow::anyhow!(e)),
        }
    }
    let digest = code.wrap(hasher.finalize())?;
    Ok(digest.to_bytes())
}

create_hash_functions!(Sha1);
create_hash_functions!(Sha2_256);
create_hash_functions!(Sha2_512);
create_hash_functions!(Sha3_224);
create_hash_functions!(Sha3_256);
create_hash_functions!(Sha3_384);
create_hash_functions!(Sha3_512);
create_hash_functions!(Keccak224);
create_hash_functions!(Keccak256);
create_hash_functions!(Keccak384);
create_hash_functions!(Keccak512);
create_hash_functions!(Blake2b256);
create_hash_functions!(Blake2b512);
create_hash_functions!(Blake2s128);
create_hash_functions!(Blake2s256);
create_hash_functions!(Blake3_256);

#[cfg(test)]
mod test {
    //
    use crate::crypto::multihash::*;
    #[test]
    fn sha1_multihash_test() -> anyhow::Result<()> {
        let hash = sha1_multihash_slice(b"Hello, World!")?;

        assert_eq!(
            bs58::encode(&hash).into_string(),
            String::from("5dqvXR93VnV1Grn96DBGdqEQAbJe1e")
        );
        Ok(())
    }

    #[test]
    fn sha2_256_multihash_test() -> anyhow::Result<()> {
        let hash = sha2_256_multihash_slice(b"Hello, World!")?;

        assert_eq!(
            bs58::encode(&hash).into_string(),
            String::from("QmdR1iHsUocy7pmRHBhNa9znM8eh8Mwqq5g5vcw8MDMXTt")
        );
        Ok(())
    }
}
