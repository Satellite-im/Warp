use std::{error::Error, path::Path};

use base64::{
    alphabet::STANDARD,
    engine::{general_purpose::PAD, GeneralPurpose},
    Engine,
};
use rust_ipfs::{Keypair, PeerId};
use serde::Deserialize;
use zeroize::Zeroizing;

#[derive(Clone, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct IpfsConfig {
    pub identity: Identity,
}

impl IpfsConfig {
    pub async fn load<P: AsRef<Path>>(path: P) -> Result<Self, Box<dyn Error>> {
        let file = tokio::fs::read(path).await?;
        let config = serde_json::from_slice(&file)?;
        Ok(config)
    }
}

#[derive(Deserialize, Clone)]
#[serde(rename_all = "PascalCase")]
pub struct Identity {
    #[serde(rename = "PeerID")]
    pub peer_id: PeerId,
    pub priv_key: String,
}

impl Identity {
    pub fn keypair(&self) -> Result<Keypair, Box<dyn Error>> {
        let engine = GeneralPurpose::new(&STANDARD, PAD);
        let keypair_bytes = Zeroizing::new(engine.decode(self.priv_key.as_bytes())?);
        let keypair = Keypair::from_protobuf_encoding(&keypair_bytes)?;
        Ok(keypair)
    }
}

impl zeroize::Zeroize for IpfsConfig {
    fn zeroize(&mut self) {
        self.identity.priv_key.zeroize();
    }
}
