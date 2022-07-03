// Used to ignore unused variables, mostly related to ones in the trait functions
//TODO: Remove
#![allow(unused_variables)]
use std::any::Any;
use std::path::PathBuf;
use warp::data::{DataObject, DataType};
use warp::pocket_dimension::query::QueryBuilder;
use warp::sync::{Arc, Mutex, MutexGuard};

use warp::module::Module;
use warp::pocket_dimension::PocketDimension;
use warp::tesseract::Tesseract;
use warp::{Extension, SingleHandle};

use ipfs::{Ipfs, IpfsOptions, Keypair, TestTypes, Types, UninitializedIpfs};
use warp::crypto::PublicKey;
use warp::error::Error;
use warp::multipass::identity::{FriendRequest, Identifier, Identity, IdentityUpdate};
use warp::multipass::{Friends, MultiPass};

#[derive(Default, Debug, Clone, Copy)]
pub enum IpfsModeType {
    #[default]
    Memory,
    Persistent,
}

#[derive(Debug, Clone)]
pub enum IpfsMode {
    Memory(Ipfs<TestTypes>),
    Persistent(Ipfs<Types>),
}

impl IpfsMode {
    pub async fn memory(opts: IpfsOptions) -> anyhow::Result<IpfsMode> {
        let (ipfs, fut): (_, _) = UninitializedIpfs::new(opts).start().await?;
        tokio::task::spawn(fut);
        Ok(IpfsMode::Memory(ipfs))
    }

    pub async fn persistent(opts: IpfsOptions) -> anyhow::Result<IpfsMode> {
        let (ipfs, fut): (_, _) = UninitializedIpfs::new(opts).start().await?;
        tokio::task::spawn(fut);
        Ok(IpfsMode::Persistent(ipfs))
    }
}

impl AsRef<Ipfs<TestTypes>> for IpfsMode {
    fn as_ref(&self) -> &Ipfs<TestTypes> {
        match self {
            IpfsMode::Memory(ipfs) => ipfs,
            IpfsMode::Persistent(_) => unreachable!(),
        }
    }
}

impl AsRef<Ipfs<Types>> for IpfsMode {
    fn as_ref(&self) -> &Ipfs<Types> {
        match self {
            IpfsMode::Memory(_) => unreachable!(),
            IpfsMode::Persistent(ipfs) => ipfs,
        }
    }
}

pub struct IpfsIdentity {
    pub cache: Option<Arc<Mutex<Box<dyn PocketDimension>>>>,
    pub tesseract: Tesseract,
    pub mode: IpfsMode,
    //TODO: FriendStore
    //      * Add/Remove/Block friends
    //      * Show incoming/outgoing request
    //TODO: AccountManager
    //      * Account registry (for self)
    //      * Account lookup
    //      * Profile information
}

impl IpfsIdentity {
    pub async fn memory(
        tesseract: Tesseract,
        cache: Option<Arc<Mutex<Box<dyn PocketDimension>>>>,
    ) -> anyhow::Result<IpfsIdentity> {
        IpfsIdentity::new(IpfsModeType::Memory, None, tesseract, cache).await
    }

    pub async fn persistent<P: AsRef<std::path::Path>>(
        path: P,
        tesseract: Tesseract,
        cache: Option<Arc<Mutex<Box<dyn PocketDimension>>>>,
    ) -> anyhow::Result<IpfsIdentity> {
        let path = path.as_ref();
        IpfsIdentity::new(
            IpfsModeType::Persistent,
            Some(path.to_path_buf()),
            tesseract,
            cache,
        )
        .await
    }

    pub async fn new(
        ipfs: IpfsModeType,
        path: Option<PathBuf>,
        tesseract: Tesseract,
        cache: Option<Arc<Mutex<Box<dyn PocketDimension>>>>,
    ) -> anyhow::Result<IpfsIdentity> {
        let opts = IpfsOptions {
            ipfs_path: path.unwrap_or_default(),
            keypair: Keypair::generate_ed25519(),
            bootstrap: vec![],
            mdns: false,
            kad_protocol: None,
            listening_addrs: vec!["/ip4/0.0.0.0/tcp/0".parse().unwrap()],
            span: None,
        };

        let mode = match ipfs {
            IpfsModeType::Memory => IpfsMode::memory(opts).await?,
            IpfsModeType::Persistent => IpfsMode::persistent(opts).await?,
        };

        //TODO: Manually load bootstrap
        match &mode {
            IpfsMode::Memory(ipfs) => ipfs.restore_bootstrappers().await?,
            IpfsMode::Persistent(ipfs) => ipfs.restore_bootstrappers().await?,
        };

        Ok(IpfsIdentity {
            tesseract,
            cache,
            mode,
        })
    }

    pub fn get_cache(&self) -> anyhow::Result<MutexGuard<Box<dyn PocketDimension>>> {
        let cache = self
            .cache
            .as_ref()
            .ok_or(Error::PocketDimensionExtensionUnavailable)?;

        Ok(cache.lock())
    }
}

impl Extension for IpfsIdentity {
    fn id(&self) -> String {
        "warp-mp-ipfs".to_string()
    }
    fn name(&self) -> String {
        "Ipfs Identity".into()
    }

    fn module(&self) -> Module {
        Module::Accounts
    }
}

impl SingleHandle for IpfsIdentity {
    fn handle(&self) -> Result<Box<dyn Any>, Error> {
        match self.mode.clone() {
            IpfsMode::Memory(ipfs) => Ok(Box::new(ipfs)),
            IpfsMode::Persistent(ipfs) => Ok(Box::new(ipfs)),
        }
    }
}

impl MultiPass for IpfsIdentity {
    fn create_identity(
        &mut self,
        username: Option<&str>,
        passphrase: Option<&str>,
    ) -> Result<PublicKey, Error> {
        todo!()
    }

    fn get_identity(&self, id: Identifier) -> Result<Identity, Error> {
        match id.get_inner() {
            (Some(_), None, false) => {}
            (None, Some(_), false) => {}
            (None, None, true) => {}
            _ => return Err(Error::InvalidIdentifierCondition),
        }
        todo!()
    }

    fn update_identity(&mut self, option: IdentityUpdate) -> Result<(), Error> {
        let mut identity = self.get_own_identity()?;
        let old_identity = identity.clone();
        match (
            option.username(),
            option.graphics_picture(),
            option.graphics_banner(),
            option.status_message(),
        ) {
            (Some(username), None, None, None) => identity.set_username(&username),
            (None, Some(hash), None, None) => {
                let mut graphics = identity.graphics();
                graphics.set_profile_picture(&hash);
                identity.set_graphics(graphics);
            }
            (None, None, Some(hash), None) => {
                let mut graphics = identity.graphics();
                graphics.set_profile_banner(&hash);
                identity.set_graphics(graphics);
            }
            (None, None, None, Some(status)) => identity.set_status_message(status),
            _ => return Err(Error::CannotUpdateIdentity),
        }

        if let Ok(mut cache) = self.get_cache() {
            let mut query = QueryBuilder::default();
            query.r#where("username", &old_identity.username())?;
            if let Ok(list) = cache.get_data(DataType::from(Module::Accounts), Some(&query)) {
                //get last
                if !list.is_empty() {
                    let mut obj = list.last().unwrap().clone();
                    obj.set_payload(identity.clone())?;
                    cache.add_data(DataType::from(Module::Accounts), &obj)?;
                }
            } else {
                cache.add_data(
                    DataType::from(Module::Accounts),
                    &DataObject::new(DataType::from(Module::Accounts), identity.clone())?,
                )?;
            }
        }

        // if let Ok(hooks) = self.get_hooks() {
        //     let object = DataObject::new(DataType::Accounts, identity.clone())?;
        //     hooks.trigger("accounts::update_identity", &object);
        // }

        Ok(())
    }

    fn decrypt_private_key(&self, passphrase: Option<&str>) -> Result<Vec<u8>, Error> {
        todo!()
    }

    fn refresh_cache(&mut self) -> Result<(), Error> {
        self.get_cache()?.empty(DataType::from(self.module()))
    }
}

impl Friends for IpfsIdentity {
    fn send_request(&mut self, pubkey: PublicKey) -> Result<(), Error> {
        todo!()
    }

    fn accept_request(&mut self, pubkey: PublicKey) -> Result<(), Error> {
        todo!()
    }

    fn deny_request(&mut self, pubkey: PublicKey) -> Result<(), Error> {
        todo!()
    }

    fn close_request(&mut self, pubkey: PublicKey) -> Result<(), Error> {
        todo!()
    }

    fn list_incoming_request(&self) -> Result<Vec<FriendRequest>, Error> {
        todo!()
    }

    fn list_outgoing_request(&self) -> Result<Vec<FriendRequest>, Error> {
        todo!()
    }

    fn list_all_request(&self) -> Result<Vec<FriendRequest>, Error> {
        todo!()
    }

    fn remove_friend(&mut self, pubkey: PublicKey) -> Result<(), Error> {
        todo!()
    }

    fn block_key(&mut self, pubkey: PublicKey) -> Result<(), Error> {
        todo!()
    }

    fn list_friends(&self) -> Result<Vec<Identity>, Error> {
        todo!()
    }

    fn has_friend(&self, pubkey: PublicKey) -> Result<(), Error> {
        todo!()
    }
}
