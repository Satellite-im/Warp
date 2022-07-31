pub mod config;
pub mod solana;
pub mod store;

use anyhow::anyhow;
use config::MpSolanaConfig;
use ipfs::{IpfsOptions, IpfsTypes, TestTypes, Types, UninitializedIpfs};
use sata::Sata;
use warp::crypto::did_key::Generate;
use warp::crypto::{rand::Rng, DID};
use warp::crypto::{DIDKey, Ed25519KeyPair, KeyMaterial};
use warp::data::{DataObject, DataType};
use warp::error::Error;
use warp::hooks::Hooks;
use warp::module::Module;
use warp::multipass::generator::generate_name;
use warp::multipass::{identity::*, Friends, MultiPass};
use warp::pocket_dimension::query::QueryBuilder;
use warp::pocket_dimension::PocketDimension;
use warp::sync::{Arc, Mutex, MutexGuard};
use warp::tesseract::Tesseract;
use warp::{async_block_in_place_uncheck, async_spawn, Extension, SingleHandle};

use crate::store::friends::FriendsStore;

use crate::solana::anchor_client::Cluster;
use crate::solana::helper::user::UserHelper;
use crate::solana::manager::SolanaManager;
use crate::solana::wallet::{PhraseType, SolanaWallet};
use anchor_client::solana_sdk::pubkey::Pubkey;
use anchor_client::solana_sdk::signature::Keypair;

pub type Temporary = TestTypes;
pub type Persistent = Types;
pub struct SolanaAccount<T: IpfsTypes> {
    pub endpoint: Cluster,
    pub cache: Option<Arc<Mutex<Box<dyn PocketDimension>>>>,
    pub tesseract: Tesseract,
    pub friend_store: Option<FriendsStore<T>>,
    pub config: MpSolanaConfig,
    pub hooks: Option<Hooks>,
}

impl<T: IpfsTypes> Default for SolanaAccount<T> {
    fn default() -> Self {
        Self {
            endpoint: Cluster::Devnet,
            cache: None,
            tesseract: Tesseract::default(),
            config: Default::default(),
            friend_store: None,
            hooks: None,
        }
    }
}

impl<T: IpfsTypes> SolanaAccount<T> {
    pub fn new(
        endpoint: Cluster,
        tesseract: &Tesseract,
        config: MpSolanaConfig,
    ) -> Result<Self, Error> {
        let tesseract = tesseract.clone();

        let mut account = Self {
            tesseract,
            endpoint,
            config,
            ..Default::default()
        };

        if let Err(_e) = account.try_init_friend_store(false) {}

        Ok(account)
    }

    pub fn with_devnet(
        tesseract: &Tesseract,
        config: Option<MpSolanaConfig>,
    ) -> Result<Self, Error> {
        let config = config.unwrap_or(MpSolanaConfig::development());
        Self::new(Cluster::Devnet, tesseract, config)
    }

    pub fn with_mainnet(
        tesseract: &Tesseract,
        config: Option<MpSolanaConfig>,
    ) -> Result<Self, Error> {
        let config = config.unwrap_or(MpSolanaConfig::production());
        Self::new(Cluster::Mainnet, tesseract, config)
    }

    pub fn with_testnet(
        tesseract: &Tesseract,
        config: Option<MpSolanaConfig>,
    ) -> Result<Self, Error> {
        let config = config.unwrap_or(MpSolanaConfig::development());
        Self::new(Cluster::Testnet, tesseract, config)
    }

    pub fn with_localnet(
        tesseract: &Tesseract,
        config: Option<MpSolanaConfig>,
    ) -> Result<Self, Error> {
        let config = config.unwrap_or(MpSolanaConfig::development());
        Self::new(Cluster::Localnet, tesseract, config)
    }

    pub fn with_custom(
        url: &str,
        ws: &str,
        tesseract: &Tesseract,
        config: MpSolanaConfig,
    ) -> Result<Self, Error> {
        Self::new(
            Cluster::Custom(url.to_string(), ws.to_string()),
            tesseract,
            config,
        )
    }

    pub fn set_cache(&mut self, cache: Arc<Mutex<Box<dyn PocketDimension>>>) {
        self.cache = Some(cache);
    }

    pub fn set_hook(&mut self, hooks: &Hooks) {
        self.hooks = Some(hooks.clone())
    }

    pub fn try_init_friend_store(&mut self, init: bool) -> anyhow::Result<()> {
        if self.friend_store().is_ok() {
            anyhow::bail!("Friend store already initialized")
        }

        let config = self.config.clone();
        let kp = self.get_private_key()?;
        let secret =
            libp2p::identity::ed25519::SecretKey::from_bytes(kp.secret().as_bytes().to_vec())?;
        let keypair = libp2p::identity::Keypair::Ed25519(secret.into());

        let mut opts = IpfsOptions {
            keypair,
            bootstrap: config.ipfs_setting.bootstrap.clone(),
            mdns: config.ipfs_setting.mdns.enable,
            listening_addrs: config.ipfs_setting.listen_on,
            dcutr: config.ipfs_setting.dcutr.enable,
            relay: config.ipfs_setting.relay_client.enable,
            relay_server: config.ipfs_setting.relay_server.enable,
            relay_addr: config.ipfs_setting.relay_client.relay_address,
            ..Default::default()
        };

        if std::any::TypeId::of::<T>() == std::any::TypeId::of::<Persistent>() {
            let path = config
                .ipfs_setting
                .path
                .as_ref()
                .ok_or_else(|| anyhow::anyhow!("\"path\" must be set"))?;
            opts.ipfs_path = path.clone();
            if !path.exists() {
                std::fs::create_dir(path)?;
            }
        }

        let (ipfs, fut) = async_block_in_place_uncheck(UninitializedIpfs::new(opts).start())?;
        async_spawn(fut);

        let friend_store = async_block_in_place_uncheck(FriendsStore::new(
            ipfs,
            self.tesseract.clone(),
            config.ipfs_setting.store_setting.discovery,
            config.ipfs_setting.store_setting.broadcast_with_connection,
            config.ipfs_setting.store_setting.broadcast_interval,
            init,
        ))?;

        self.friend_store = Some(friend_store);

        Ok(())
    }

    pub fn insert_solana_wallet(&mut self, wallet: SolanaWallet) -> anyhow::Result<()> {
        let mut tesseract = self.get_tesseract()?;

        tesseract.set("mnemonic", &wallet.get_mnemonic_phrase()?)?;
        tesseract.set("keypair", wallet.get_keypair()?.to_base58_string().as_str())?;

        Ok(())
    }

    pub fn get_private_key(&self) -> anyhow::Result<Keypair> {
        let mut tesseract = self.get_tesseract()?;

        let private_key = match tesseract.retrieve("keypair") {
            Ok(key) => key,
            Err(e) => {
                if tesseract.exist("mnemonic") {
                    let mnemonic = tesseract.retrieve("mnemonic")?;
                    let wallet = SolanaWallet::restore_from_mnemonic(None, &mnemonic)?;
                    let kp = wallet.get_keypair()?.to_base58_string();
                    tesseract.set("keypair", kp.as_str())?;
                    kp
                } else {
                    return Err(anyhow!(e));
                }
            }
        };

        let keypair = Keypair::from_base58_string(private_key.as_str());
        Ok(keypair)
    }

    pub fn get_wallet(&self) -> Result<SolanaWallet, Error> {
        let tesseract = self.get_tesseract()?;
        let mnemonic = tesseract.retrieve("mnemonic")?;
        SolanaWallet::restore_from_mnemonic(None, &mnemonic)
    }

    pub fn get_tesseract(&self) -> anyhow::Result<Tesseract> {
        Ok(self.tesseract.clone())
    }

    pub fn get_cache(&self) -> anyhow::Result<MutexGuard<Box<dyn PocketDimension>>> {
        let cache = self
            .cache
            .as_ref()
            .ok_or(Error::PocketDimensionExtensionUnavailable)?;

        Ok(cache.lock())
    }

    pub fn get_hooks(&self) -> anyhow::Result<&Hooks> {
        self.hooks
            .as_ref()
            .ok_or_else(|| anyhow!("Hooks is not set"))
    }

    pub fn user_helper(&self) -> anyhow::Result<UserHelper> {
        let kp = self.get_private_key()?;
        let helper = UserHelper::new_with_cluster(self.endpoint.clone(), &kp);
        Ok(helper)
    }

    pub fn friend_store(&self) -> anyhow::Result<&FriendsStore<T>> {
        self.friend_store
            .as_ref()
            .ok_or(anyhow::anyhow!("Friends store is not initialized"))
    }

    pub fn friend_store_mut(&mut self) -> anyhow::Result<&mut FriendsStore<T>> {
        self.friend_store
            .as_mut()
            .ok_or(anyhow::anyhow!("Friends store is not initialized"))
    }
}

impl<T: IpfsTypes> Extension for SolanaAccount<T> {
    fn id(&self) -> String {
        String::from("warp-mp-solana")
    }

    fn name(&self) -> String {
        String::from("Solana Multipass")
    }

    fn module(&self) -> Module {
        Module::Accounts
    }
}

impl<T: IpfsTypes> SingleHandle for SolanaAccount<T> {
    fn handle(&self) -> Result<Box<dyn core::any::Any>, Error> {
        if let Some(store) = self.friend_store.as_ref() {
            return Ok(Box::new(store.ipfs()));
        }
        Err(Error::Unimplemented)
    }
}

impl<T: IpfsTypes> MultiPass for SolanaAccount<T> {
    fn create_identity(&mut self, username: Option<&str>, _: Option<&str>) -> Result<DID, Error> {
        if let Ok(keypair) = &self.get_private_key() {
            if UserHelper::new_with_cluster(self.endpoint.clone(), keypair)
                .get_current_user()
                .is_ok()
            {
                return Err(Error::IdentityExist);
            }
        }

        let username = match username {
            Some(u) => u.to_string(),
            None => generate_name(),
        };

        let wallet = match self.get_wallet() {
            Ok(wallet) => wallet,
            Err(_) => SolanaWallet::create_random(PhraseType::Standard, None)?,
        };

        let mut helper =
            UserHelper::new_with_cluster(self.endpoint.clone(), &wallet.get_keypair()?);

        if let Ok(identity) = user_to_identity(&helper, None) {
            if identity.username() == username {
                return Err(Error::IdentityExist);
            }
        }

        let manager = SolanaManager::new(self.endpoint.clone(), &wallet)?;

        if manager.get_account_balance()? == 0 {
            manager.request_air_drop()?;
        }
        let code: i32 = warp::crypto::rand::thread_rng().gen_range(0, 9999);

        let uname = format!("{username}#{}", code);

        helper.create(&uname, "", "We have lift off")?;

        //Note: This is used so that we can obtain the public key when we look up an account by username
        let pubkey = wallet.get_pubkey()?;
        helper.set_extra_one(&pubkey.to_string())?;

        if self.get_wallet().is_err() {
            self.insert_solana_wallet(wallet)?;
        }

        let identity = user_to_identity(&helper, None)?;

        if let Ok(mut cache) = self.get_cache() {
            let object = Sata::default().encode(
                warp::sata::libipld::IpldCodec::DagJson,
                warp::sata::Kind::Reference,
                identity.clone(),
            )?;
            cache.add_data(DataType::from(Module::Accounts), &object)?;
        }
        if let Ok(hooks) = self.get_hooks() {
            let object = DataObject::new(DataType::Accounts, identity.clone())?;
            hooks.trigger("accounts::new_identity", &object);
        }

        self.try_init_friend_store(true)?;

        Ok(identity.did_key())
    }

    fn get_identity(&self, id: Identifier) -> Result<Identity, Error> {
        let helper = self.user_helper()?;
        let ident = match id.get_inner() {
            (None, Some(username), false) => {
                if let Ok(cache) = self.get_cache() {
                    let mut query = QueryBuilder::default();
                    query.r#where("username", &username)?;
                    if let Ok(list) = cache.get_data(DataType::from(Module::Accounts), Some(&query))
                    {
                        //get last
                        if !list.is_empty() {
                            let obj = list.last().unwrap();
                            return obj.decode::<Identity>().map_err(Error::from);
                        }
                    }
                }
                let user = helper.get_user_by_name(&username)?;
                // If this field is empty or cannot be decoded from base58 we should return an error
                // Note: We may have to cross check the public key against the account that holds it
                //       for security purpose
                if user.extra_1.is_empty() {
                    return Err(Error::Any(anyhow!(
                        "Unable to obtain identity by username (Incompatible Account)"
                    )));
                }

                match bs58::decode(&user.extra_1)
                    .into_vec()
                    .map_err(|e| anyhow!(e))
                {
                    Ok(pkey) => {
                        if pkey.is_empty() || pkey.len() < 31 {
                            return Err(Error::InvalidPublicKeyLength);
                        }
                        user_to_identity(&helper, Some(&pkey))?
                    }
                    Err(e) => return Err(Error::Any(e)),
                }
            }
            (Some(pkey), None, false) => {
                if let Ok(cache) = self.get_cache() {
                    let mut query = QueryBuilder::default();
                    query.r#where("did_key", &pkey)?;
                    if let Ok(list) = cache.get_data(DataType::from(Module::Accounts), Some(&query))
                    {
                        //get last
                        if !list.is_empty() {
                            let obj = list.last().unwrap();
                            return obj.decode::<Identity>().map_err(Error::from);
                        }
                    }
                }
                let did: DIDKey = pkey.try_into()?;

                user_to_identity(&helper, Some(&did.public_key_bytes()))?
            }
            (None, None, true) => user_to_identity(&helper, None)?,
            _ => return Err(Error::InvalidIdentifierCondition),
        };

        if let Ok(mut cache) = self.get_cache() {
            let mut query = QueryBuilder::default();
            query.r#where("did_key", &ident.did_key())?;
            if cache
                .has_data(DataType::from(Module::Accounts), &query)
                .is_err()
            {
                let object = Sata::default().encode(
                    warp::sata::libipld::IpldCodec::DagJson,
                    warp::sata::Kind::Reference,
                    ident.clone(),
                )?;
                cache.add_data(DataType::from(Module::Accounts), &object)?;
            }
        }
        Ok(ident)
    }

    fn update_identity(&mut self, option: IdentityUpdate) -> Result<(), Error> {
        let mut helper = self.user_helper()?;

        let mut identity = user_to_identity(&helper, None)?;
        let old_identity = identity.clone();
        match (
            option.username(),
            option.graphics_picture(),
            option.graphics_banner(),
            option.status_message(),
        ) {
            (Some(username), None, None, None) => {
                helper
                    .set_name(&format!("{username}#{}", identity.short_id()))
                    .map_err(|_| Error::CannotUpdateIdentityUsername)?;
                identity.set_username(&username)
            }
            (None, Some(hash), None, None) => {
                helper
                    .set_photo(&hash)
                    .map_err(|_| Error::CannotUpdateIdentityPicture)?;
                let mut graphics = identity.graphics();
                graphics.set_profile_picture(&hash);
                identity.set_graphics(graphics);
            }
            (None, None, Some(hash), None) => {
                helper
                    .set_banner_image(&hash)
                    .map_err(|_| Error::CannotUpdateIdentityBanner)?;
                let mut graphics = identity.graphics();
                graphics.set_profile_banner(&hash);
                identity.set_graphics(graphics);
            }
            (None, None, None, Some(status)) => {
                helper
                    .set_status(&status.clone().unwrap_or_default())
                    .map_err(|_| Error::CannotUpdateIdentityStatus)?;
                identity.set_status_message(status)
            }
            _ => return Err(Error::CannotUpdateIdentity),
        }

        if let Ok(mut cache) = self.get_cache() {
            let mut query = QueryBuilder::default();
            query.r#where("username", &old_identity.username())?;
            if let Ok(list) = cache.get_data(DataType::from(Module::Accounts), Some(&query)) {
                //get last
                if !list.is_empty() {
                    let mut object = Sata::default();
                    object.set_version(list.len() as _);
                    let obj = object.encode(
                        warp::sata::libipld::IpldCodec::DagJson,
                        warp::sata::Kind::Reference,
                        identity.clone(),
                    )?;
                    cache.add_data(DataType::from(Module::Accounts), &obj)?;
                }
            } else {
                let object = Sata::default().encode(
                    warp::sata::libipld::IpldCodec::DagJson,
                    warp::sata::Kind::Reference,
                    identity.clone(),
                )?;
                cache.add_data(DataType::from(Module::Accounts), &object)?;
            }
        }

        if let Ok(hooks) = self.get_hooks() {
            let object = DataObject::new(DataType::Accounts, identity.clone())?;
            hooks.trigger("accounts::update_identity", &object);
        }

        Ok(())
    }

    fn decrypt_private_key(&self, _: Option<&str>) -> Result<DID, Error> {
        let keypair = self.get_private_key()?;
        let did = DIDKey::Ed25519(Ed25519KeyPair::from_secret_key(keypair.secret().as_bytes()));
        Ok(did.into())
    }

    fn refresh_cache(&mut self) -> Result<(), Error> {
        self.get_cache()?.empty(DataType::from(self.module()))
    }
}

impl<T: IpfsTypes> Friends for SolanaAccount<T> {
    fn send_request(&mut self, pubkey: &DID) -> Result<(), Error> {
        async_block_in_place_uncheck(self.friend_store_mut()?.send_request(&pubkey))?;
        if let Ok(hooks) = self.get_hooks() {
            if let Some(request) = self
                .list_outgoing_request()?
                .iter()
                .filter(|request| request.to().eq(pubkey))
                .collect::<Vec<_>>()
                .first()
            {
                let object = DataObject::new(DataType::Accounts, request)?;
                hooks.trigger("accounts::send_friend_request", &object);
            }
        }
        Ok(())
    }

    fn accept_request(&mut self, pubkey: &DID) -> Result<(), Error> {
        async_block_in_place_uncheck(self.friend_store_mut()?.accept_request(&pubkey))?;
        if let Ok(hooks) = self.get_hooks() {
            if let Some(key) = self
                .list_friends()?
                .iter()
                .filter(|pk| pk.eq(&pubkey))
                .collect::<Vec<_>>()
                .first()
            {
                let object = DataObject::new(DataType::Accounts, key)?;
                hooks.trigger("accounts::accept_friend_request", &object);
            }
        }
        Ok(())
    }

    fn deny_request(&mut self, pubkey: &DID) -> Result<(), Error> {
        async_block_in_place_uncheck(self.friend_store_mut()?.reject_request(&pubkey))?;
        if let Ok(hooks) = self.get_hooks() {
            if !self
                .list_all_request()?
                .iter()
                .any(|request| request.from().eq(pubkey))
            {
                let object = DataObject::new(DataType::Accounts, ())?;
                hooks.trigger("accounts::deny_friend_request", &object);
            }
        }
        Ok(())
    }

    fn list_incoming_request(&self) -> Result<Vec<FriendRequest>, Error> {
        Ok(self.friend_store()?.list_incoming_request())
    }

    fn list_outgoing_request(&self) -> Result<Vec<FriendRequest>, Error> {
        Ok(self.friend_store()?.list_outgoing_request())
    }

    fn list_all_request(&self) -> Result<Vec<FriendRequest>, Error> {
        Ok(self.friend_store()?.list_all_request())
    }

    fn remove_friend(&mut self, pubkey: &DID) -> Result<(), Error> {
        async_block_in_place_uncheck(self.friend_store_mut()?.remove_friend(&pubkey, true))?;
        if let Ok(hooks) = self.get_hooks() {
            if self.has_friend(pubkey).is_err() {
                let object = DataObject::new(DataType::Accounts, pubkey)?;
                hooks.trigger("accounts::remove_friend", &object);
            }
        }
        Ok(())
    }

    fn block(&mut self, pubkey: &DID) -> Result<(), Error> {
        async_block_in_place_uncheck(self.friend_store_mut()?.block(&pubkey))?;
        if let Ok(hooks) = self.get_hooks() {
            if self.has_friend(pubkey).is_err() {
                let object = DataObject::new(DataType::Accounts, pubkey)?;
                hooks.trigger("accounts::block_key", &object);
            }
        }
        Ok(())
    }

    fn unblock(&mut self, pubkey: &DID) -> Result<(), Error> {
        async_block_in_place_uncheck(self.friend_store_mut()?.unblock(&pubkey))?;
        if let Ok(hooks) = self.get_hooks() {
            if self.has_friend(pubkey).is_err() {
                let object = DataObject::new(DataType::Accounts, pubkey)?;
                hooks.trigger("accounts::unblock_key", &object);
            }
        }
        Ok(())
    }

    fn block_list(&self) -> Result<Vec<DID>, Error> {
        async_block_in_place_uncheck(self.friend_store()?.block_list())
    }

    fn list_friends(&self) -> Result<Vec<DID>, Error> {
        async_block_in_place_uncheck(self.friend_store()?.friends_list())
    }

    fn has_friend(&self, pubkey: &DID) -> Result<(), Error> {
        async_block_in_place_uncheck(self.friend_store()?.is_friend(&pubkey))
    }
}

fn user_to_identity(helper: &UserHelper, pubkey: Option<&[u8]>) -> anyhow::Result<Identity> {
    let (user, pubkey) = match pubkey {
        Some(pubkey) => {
            let pkey = Pubkey::new(pubkey);
            let user = helper.get_user(pkey)?;
            (user, pkey)
        }
        None => {
            let user = helper.get_current_user()?;
            let pkey = helper.program.payer();
            (user, pkey)
        }
    };

    let mut identity = Identity::default();
    //Note: This is temporary
    if user.name.contains('#') {
        let split_data = user.name.split('#').collect::<Vec<&str>>();

        if split_data.len() != 2 {
            //Because of it being invalid and due to the lack of short code within the contract
            //we will not error here but instead would ignore and return the original username to
            //the identity.
            identity.set_username(&user.name);
        } else {
            match (
                split_data.get(0).ok_or(Error::Other).map(|s| s.to_string()),
                split_data.get(1).ok_or(Error::Other)?.parse(),
            ) {
                (Ok(name), Ok(code)) => {
                    identity.set_username(&name);
                    identity.set_short_id(code);
                }
                _ => identity.set_username(&user.name),
            };
        }
    } else {
        identity.set_username(&user.name);
    };

    let did: DIDKey = Ed25519KeyPair::from_public_key(&pubkey.to_bytes()).into();
    identity.set_did_key(did.into());
    identity.set_status_message(Some(user.status));
    let mut graphics = Graphics::default();
    graphics.set_profile_banner(&user.banner_image_hash);
    graphics.set_profile_picture(&user.photo_hash);
    identity.set_graphics(graphics);
    Ok(identity)
}

pub mod ffi {
    use std::ffi::CStr;
    use std::os::raw::c_char;

    use warp::error::Error;
    use warp::ffi::FFIResult;
    use warp::multipass::MultiPassAdapter;
    use warp::pocket_dimension::PocketDimensionAdapter;
    use warp::sync::{Arc, Mutex};
    use warp::tesseract::Tesseract;

    use crate::{Persistent, SolanaAccount, Temporary};

    #[allow(clippy::missing_safety_doc)]
    #[no_mangle]
    pub unsafe extern "C" fn multipass_mp_solana_temporary_with_devnet(
        pocketdimension: *const PocketDimensionAdapter,
        tesseract: *const Tesseract,
        config: *const c_char,
    ) -> FFIResult<MultiPassAdapter> {
        let tesseract = match tesseract.is_null() {
            false => {
                let tesseract = &*tesseract;
                tesseract.clone()
            }
            true => Tesseract::default(),
        };

        let config = match config.is_null() {
            true => None,
            false => {
                let config = CStr::from_ptr(config).to_string_lossy().to_string();
                match serde_json::from_str(&config) {
                    Ok(c) => Some(c),
                    Err(e) => return FFIResult::err(Error::from(e)),
                }
            }
        };

        let mut account = match SolanaAccount::<Temporary>::with_devnet(&tesseract, config) {
            Ok(account) => account,
            Err(e) => return FFIResult::err(e),
        };

        match pocketdimension.is_null() {
            true => {}
            false => {
                let pd = &*pocketdimension;
                account.set_cache(pd.inner().clone());
            }
        }

        let mp = MultiPassAdapter::new(Arc::new(Mutex::new(Box::new(account))));
        FFIResult::ok(mp)
    }

    #[allow(clippy::missing_safety_doc)]
    #[no_mangle]
    pub unsafe extern "C" fn multipass_mp_solana_persistent_with_devnet(
        pocketdimension: *const PocketDimensionAdapter,
        tesseract: *const Tesseract,
        config: *const c_char,
    ) -> FFIResult<MultiPassAdapter> {
        let tesseract = match tesseract.is_null() {
            false => {
                let tesseract = &*tesseract;
                tesseract.clone()
            }
            true => Tesseract::default(),
        };

        let config = match config.is_null() {
            true => None,
            false => {
                let config = CStr::from_ptr(config).to_string_lossy().to_string();
                match serde_json::from_str(&config) {
                    Ok(c) => Some(c),
                    Err(e) => return FFIResult::err(Error::from(e)),
                }
            }
        };

        let mut account = match SolanaAccount::<Persistent>::with_devnet(&tesseract, config) {
            Ok(account) => account,
            Err(e) => return FFIResult::err(e),
        };

        match pocketdimension.is_null() {
            true => {}
            false => {
                let pd = &*pocketdimension;
                account.set_cache(pd.inner().clone());
            }
        }

        let mp = MultiPassAdapter::new(Arc::new(Mutex::new(Box::new(account))));
        FFIResult::ok(mp)
    }

    #[allow(clippy::missing_safety_doc)]
    #[no_mangle]
    pub unsafe extern "C" fn multipass_mp_solana_new_with_testnet(
        pocketdimension: *const PocketDimensionAdapter,
        tesseract: *const Tesseract,
        config: *const c_char,
    ) -> FFIResult<MultiPassAdapter> {
        let tesseract = match tesseract.is_null() {
            false => {
                let tesseract = &*tesseract;
                tesseract.clone()
            }
            true => Tesseract::default(),
        };

        let config = match config.is_null() {
            true => None,
            false => {
                let config = CStr::from_ptr(config).to_string_lossy().to_string();
                match serde_json::from_str(&config) {
                    Ok(c) => Some(c),
                    Err(e) => return FFIResult::err(Error::from(e)),
                }
            }
        };

        let mut account = match SolanaAccount::<Persistent>::with_testnet(&tesseract, config) {
            Ok(account) => account,
            Err(e) => return FFIResult::err(e),
        };

        match pocketdimension.is_null() {
            true => {}
            false => {
                let pd = &*pocketdimension;
                account.set_cache(pd.inner().clone());
            }
        }

        let mp = MultiPassAdapter::new(Arc::new(Mutex::new(Box::new(account))));
        FFIResult::ok(mp)
    }

    #[allow(clippy::missing_safety_doc)]
    #[no_mangle]
    pub unsafe extern "C" fn multipass_mp_solana_new_with_mainnet(
        pocketdimension: *const PocketDimensionAdapter,
        tesseract: *const Tesseract,
        config: *const c_char,
    ) -> FFIResult<MultiPassAdapter> {
        let tesseract = match tesseract.is_null() {
            false => {
                let tesseract = &*(tesseract);
                tesseract.clone()
            }
            true => Tesseract::default(),
        };

        let config = match config.is_null() {
            true => None,
            false => {
                let config = CStr::from_ptr(config).to_string_lossy().to_string();
                match serde_json::from_str(&config) {
                    Ok(c) => Some(c),
                    Err(e) => return FFIResult::err(Error::from(e)),
                }
            }
        };

        let mut account = match SolanaAccount::<Persistent>::with_mainnet(&tesseract, config) {
            Ok(account) => account,
            Err(e) => return FFIResult::err(e),
        };

        match pocketdimension.is_null() {
            true => {}
            false => {
                let pd = &*pocketdimension;
                account.set_cache(pd.inner().clone());
            }
        }

        let mp = MultiPassAdapter::new(Arc::new(Mutex::new(Box::new(account))));
        FFIResult::ok(mp)
    }
}
