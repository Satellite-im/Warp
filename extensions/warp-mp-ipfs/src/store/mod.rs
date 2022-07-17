use serde::Serialize;
use warp::{
    crypto::{
        signature::{Ed25519Keypair, Ed25519PublicKey},
        PublicKey,
    },
    error::Error,
    tesseract::Tesseract,
};

pub mod friends;
pub mod identity;

pub const IDENTITY_BROADCAST: &str = "identity/broadcast";
pub const FRIENDS_BROADCAST: &str = "friends/broadcast";

fn pub_to_libp2p_pub(public_key: &PublicKey) -> anyhow::Result<libp2p::identity::PublicKey> {
    let pk = libp2p::identity::PublicKey::Ed25519(libp2p::identity::ed25519::PublicKey::decode(
        &public_key.into_bytes(),
    )?);
    Ok(pk)
}

fn libp2p_pub_to_pub(public_key: &libp2p::identity::PublicKey) -> anyhow::Result<PublicKey> {
    let pk = match public_key {
        libp2p::identity::PublicKey::Ed25519(pk) => PublicKey::from_bytes(&pk.encode()),
        _ => anyhow::bail!(Error::PublicKeyInvalid),
    };
    Ok(pk)
}

// Note that this are temporary
fn sign_serde<D: Serialize>(tesseract: &Tesseract, data: &D) -> anyhow::Result<Vec<u8>> {
    let kp = tesseract.retrieve("ipfs_keypair")?;
    let kp = bs58::decode(kp).into_vec()?;
    let keypair = Ed25519Keypair::from_bytes(&kp)?;
    let bytes = serde_json::to_vec(data)?;
    Ok(keypair.sign(&bytes))
}

// Note that this are temporary
fn verify_serde_sig<D: Serialize>(
    pk: Ed25519PublicKey,
    data: &D,
    signature: &[u8],
) -> anyhow::Result<()> {
    let bytes = serde_json::to_vec(data)?;
    pk.verify(&bytes, signature)?;
    Ok(())
}

// This function stores the topic as a dag in a "gossipsub:<topic>" format and provide the cid over DHT and obtain the providers of the same cid
// who are providing and connect to them.
// Note that there is usually a delay in `ipfs.provide`.
// TODO: Investigate the delay in providing the CID
pub async fn topic_discovery<S: AsRef<str>>(
    ipfs: ipfs::Ipfs<ipfs::Types>,
    topic: S,
) -> anyhow::Result<()> {
    let topic = topic.as_ref();
    let cid = ipfs
        .put_dag(libipld::ipld!(format!("gossipsub:{}", topic)))
        .await?;
    ipfs.provide(cid).await?;

    loop {
        if let Ok(list) = ipfs.get_providers(cid).await {
            for peer in list {
                // Check to see if we are already connected to the peer
                if let Ok(connections) = ipfs.peers().await {
                    if connections
                        .iter()
                        .filter(|connection| connection.addr.peer_id == peer)
                        .count()
                        >= 1
                    {
                        continue;
                    }
                }

                // Get address(es) of peer and connect to them
                // TODO: Maybe use a relay or give an option when connecting?
                if let Ok(addrs) = ipfs.find_peer(peer).await {
                    for addr in addrs {
                        let addr = addr.with(ipfs::Protocol::P2p(peer.into()));
                        if let Ok(addr) = addr.try_into() {
                            if let Err(_e) = ipfs.connect(addr).await {
                                //TODO: Log
                                continue;
                            }
                        }
                    }
                }
            }
        }
    }
}
