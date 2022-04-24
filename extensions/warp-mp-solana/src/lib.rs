use std::sync::{Arc, Mutex, MutexGuard};

use warp_common::anyhow::{anyhow, bail};
use warp_common::error::Error;
use warp_common::{anyhow, Extension, Module};
use warp_crypto::rand::Rng;
use warp_data::{DataObject, DataType};
use warp_hooks::hooks::Hooks;
use warp_multipass::{identity::*, Friends, MultiPass};
use warp_pocket_dimension::query::QueryBuilder;
use warp_pocket_dimension::PocketDimension;
use warp_solana_utils::anchor_client::solana_client::rpc_client::RpcClient;
use warp_solana_utils::anchor_client::solana_sdk::pubkey::Pubkey;
use warp_solana_utils::anchor_client::solana_sdk::signature::Keypair;
use warp_solana_utils::helper::friends::Status;
use warp_solana_utils::helper::user::UserHelper;
use warp_solana_utils::manager::SolanaManager;
use warp_solana_utils::wallet::{PhraseType, SolanaWallet};
use warp_solana_utils::{helper, EndPoint};
use warp_tesseract::Tesseract;

pub struct SolanaAccount {
    pub endpoint: EndPoint,
    pub connection: Option<RpcClient>,
    pub identity: Option<Identity>,
    pub contacts: Option<Vec<PublicKey>>,
    pub cache: Option<Arc<Mutex<Box<dyn PocketDimension>>>>,
    pub tesseract: Option<Arc<Mutex<Tesseract>>>,
    pub hooks: Option<Arc<Mutex<Hooks>>>,
}

impl Default for SolanaAccount {
    fn default() -> Self {
        Self {
            identity: None,
            connection: None,
            endpoint: EndPoint::DevNet,
            contacts: None,
            cache: None,
            tesseract: None,
            hooks: None,
        }
    }
}

impl SolanaAccount {
    pub fn new(endpoint: EndPoint) -> Self {
        let connection = Some(RpcClient::new(endpoint.to_string()));
        Self {
            endpoint,
            connection,
            ..Default::default()
        }
    }

    pub fn with_devnet() -> Self {
        Self::new(EndPoint::DevNet)
    }

    pub fn with_mainnet() -> Self {
        Self::new(EndPoint::MainNetBeta)
    }

    pub fn with_testnet() -> Self {
        Self::new(EndPoint::TestNet)
    }

    pub fn set_cache(&mut self, cache: Arc<Mutex<Box<dyn PocketDimension>>>) {
        self.cache = Some(cache);
    }

    pub fn set_hook(&mut self, hooks: Arc<Mutex<Hooks>>) {
        self.hooks = Some(hooks)
    }

    pub fn set_tesseract(&mut self, tesseract: Arc<Mutex<Tesseract>>) {
        self.tesseract = Some(tesseract)
    }

    pub fn insert_solana_wallet(&mut self, wallet: SolanaWallet) -> anyhow::Result<()> {
        let mut tesseract = self.get_tesseract()?;

        tesseract.set("mnemonic", &wallet.get_mnemonic_phrase()?)?;
        tesseract.set("privkey", wallet.get_keypair()?.to_base58_string().as_str())?;

        Ok(())
    }

    pub fn get_private_key(&self) -> anyhow::Result<Keypair> {
        let tesseract = self.get_tesseract()?;

        if !tesseract.exist("privkey") {
            bail!("Private key is not set or available");
        }

        let private_key = tesseract.retrieve("privkey")?;

        let keypair = Keypair::from_base58_string(private_key.as_str());
        Ok(keypair)
    }

    pub fn get_tesseract(&self) -> anyhow::Result<MutexGuard<Tesseract>> {
        let tesseract = self
            .tesseract
            .as_ref()
            .ok_or_else(|| anyhow!("Tesseract is not available"))?;
        let inner = match tesseract.lock() {
            Ok(inner) => inner,
            Err(e) => e.into_inner(),
        };
        Ok(inner)
    }

    pub fn get_cache(&self) -> anyhow::Result<MutexGuard<Box<dyn PocketDimension>>> {
        match self
            .cache
            .as_ref()
            .ok_or_else(|| anyhow!("Pocket Dimension Extension is not set"))?
            .lock()
        {
            Ok(inner) => Ok(inner),
            Err(e) => Ok(e.into_inner()),
        }
    }

    pub fn user_helper(&self) -> anyhow::Result<UserHelper> {
        let kp = self.get_private_key()?;
        let helper = UserHelper::new_with_keypair(&kp);
        Ok(helper)
    }

    pub fn friend_helper(&self) -> anyhow::Result<helper::friends::Friends> {
        let kp = self.get_private_key()?;
        let helper = helper::friends::Friends::new_with_keypair(&kp);
        Ok(helper)
    }
}

impl Extension for SolanaAccount {
    fn id(&self) -> String {
        String::from("warp-mp-solana")
    }

    fn name(&self) -> String {
        String::from("Solana Multipass")
    }

    fn module(&self) -> warp_common::Module {
        Module::Accounts
    }
}

impl MultiPass for SolanaAccount {
    fn create_identity(&mut self, username: &str, _: &str) -> warp_common::Result<PublicKey> {
        if let Ok(keypair) = &self.get_private_key() {
            if UserHelper::new_with_keypair(keypair)
                .get_current_user()
                .is_ok()
            {
                return Err(Error::Other);
            }
        }

        let wallet = SolanaWallet::create_random(PhraseType::Standard, None)?;
        let mut helper = UserHelper::new_with_wallet(&wallet)?;

        if let Ok(identity) = user_to_identity(&helper, None) {
            if identity.username == *username {
                return Err(Error::ToBeDetermined);
            }
        }
        let mut manager = SolanaManager::new();
        manager.initiralize_from_solana_wallet(&wallet)?;

        if manager.get_account_balance()? == 0 {
            manager.request_air_drop()?;
        }
        let code: i32 = warp_crypto::rand::thread_rng().gen_range(0, 9999);

        let uname = format!("{username}#{code}");

        helper.create(&uname, "", "We have lift off")?;

        self.insert_solana_wallet(wallet)?;

        let identity = user_to_identity(&helper, None)?;

        self.identity = Some(identity.clone());

        if let Ok(mut cache) = self.get_cache() {
            let object = DataObject::new(DataType::Module(Module::Accounts), &identity)?;
            cache.add_data(DataType::Module(Module::Accounts), &object)?;
        }
        Ok(identity.public_key)
    }

    fn get_identity(&self, id: Identifier) -> warp_common::Result<Identity> {
        let helper = self.user_helper()?;
        let ident = match id {
            Identifier::Username(username) => {
                if let Ok(cache) = self.get_cache() {
                    let mut query = QueryBuilder::default();
                    query.r#where("username", &username)?;
                    if let Ok(list) =
                        cache.get_data(DataType::Module(Module::Accounts), Some(&query))
                    {
                        //get last
                        if !list.is_empty() {
                            let obj = list.last().unwrap();
                            return obj.payload::<Identity>();
                        }
                    }
                }
                return Err(Error::Unimplemented);
            }
            Identifier::PublicKey(pkey) => {
                if let Ok(cache) = self.get_cache() {
                    let mut query = QueryBuilder::default();
                    query.r#where("public_key", &pkey)?;
                    if let Ok(list) =
                        cache.get_data(DataType::Module(Module::Accounts), Some(&query))
                    {
                        //get last
                        if !list.is_empty() {
                            let obj = list.last().unwrap();
                            return obj.payload::<Identity>();
                        }
                    }
                }
                user_to_identity(&helper, Some(pkey.to_bytes()))?
            }
            Identifier::Own => user_to_identity(&helper, None)?,
        };

        if let Ok(mut cache) = self.get_cache() {
            let mut query = QueryBuilder::default();
            query.r#where("public_key", &ident.public_key)?;
            if cache
                .has_data(DataType::Module(Module::Accounts), &query)
                .is_err()
            {
                let object = DataObject::new(DataType::Module(Module::Accounts), &ident)?;
                cache.add_data(DataType::Module(Module::Accounts), &object)?;
            }
        }
        Ok(ident)
    }

    fn update_identity(&mut self, option: IdentityUpdate) -> warp_common::Result<()> {
        let mut helper = self.user_helper()?;

        let mut identity = user_to_identity(&helper, None)?;
        let old_identity = identity.clone();
        match option {
            IdentityUpdate::Username(username) => {
                helper.set_name(&format!("{username}#{}", identity.short_id))?; //TODO: Investigate why it *sometimes* errors and causes interaction to contract to error until there is an update
                identity.username = username
            }
            IdentityUpdate::Graphics { picture, banner } => {
                if let Some(hash) = picture {
                    helper.set_photo(&hash)?;
                    identity.graphics.profile_picture = hash;
                }
                if let Some(hash) = banner {
                    helper.set_banner_image(&hash)?;
                    identity.graphics.profile_banner = hash;
                }
            }
            IdentityUpdate::StatusMessage(status) => {
                helper.set_status(&status.clone().unwrap_or_default())?;
                identity.status_message = status
            }
        }

        if let Ok(mut cache) = self.get_cache() {
            let mut query = QueryBuilder::default();
            query.r#where("username", &old_identity.username)?;
            if let Ok(list) = cache.get_data(DataType::Module(Module::Accounts), Some(&query)) {
                //get last
                if !list.is_empty() {
                    let mut obj = list.last().unwrap().clone();
                    obj.set_payload(identity.clone())?;
                    cache.add_data(DataType::Module(Module::Accounts), &obj)?;
                }
            } else {
                cache.add_data(
                    DataType::Module(Module::Accounts),
                    &DataObject::new(DataType::Module(Module::Accounts), identity.clone())?,
                )?;
            }
        }
        self.identity = Some(identity);
        Ok(())
    }

    fn decrypt_private_key(&self, _: &str) -> warp_common::Result<Vec<u8>> {
        let keypair = self.get_private_key()?;
        Ok(keypair.to_bytes().to_vec())
    }

    fn refresh_cache(&mut self) -> warp_common::Result<()> {
        self.get_cache()?.empty(DataType::Module(self.module()))
    }
}

impl Friends for SolanaAccount {
    fn send_request(&mut self, pubkey: PublicKey) -> warp_common::Result<()> {
        //check to see if account is valid
        if self
            .get_identity(Identifier::PublicKey(pubkey.clone()))
            .is_err()
        {
            return Err(Error::Unimplemented);
        }

        if self.has_friend(pubkey.clone()).is_ok() {
            return Err(Error::Unimplemented);
        }

        let helper = self.friend_helper()?;

        helper.create_friend_request(Pubkey::new(pubkey.to_bytes()), "")?;
        Ok(())
    }

    fn accept_request(&mut self, pubkey: PublicKey) -> warp_common::Result<()> {
        if self
            .get_identity(Identifier::PublicKey(pubkey.clone()))
            .is_err()
        {
            return Err(Error::Unimplemented);
        }

        if self.has_friend(pubkey.clone()).is_ok() {
            return Err(Error::Unimplemented);
        }

        let helper = self.friend_helper()?;

        helper.accept_friend_request(Pubkey::new(pubkey.to_bytes()), "")?;
        Ok(())
    }

    fn deny_request(&mut self, pubkey: PublicKey) -> warp_common::Result<()> {
        if self
            .get_identity(Identifier::PublicKey(pubkey.clone()))
            .is_err()
        {
            return Err(Error::Unimplemented);
        }

        if self.has_friend(pubkey.clone()).is_ok() {
            return Err(Error::Unimplemented);
        }

        let helper = self.friend_helper()?;

        helper.create_friend_request(Pubkey::new(pubkey.to_bytes()), "")?;
        Ok(())
    }

    fn close_request(&mut self, pubkey: PublicKey) -> warp_common::Result<()> {
        if self
            .get_identity(Identifier::PublicKey(pubkey.clone()))
            .is_err()
        {
            return Err(Error::Unimplemented);
        }

        if self.has_friend(pubkey.clone()).is_ok() {
            return Err(Error::Unimplemented);
        }

        let helper = self.friend_helper()?;

        helper.close_friend_request(Pubkey::new(pubkey.to_bytes()))?;
        Ok(())
    }

    fn list_request(&self) -> warp_common::Result<Vec<FriendRequest>> {
        self.list_all_request().map(|fr| {
            fr.iter()
                .filter(|fr| fr.status == FriendRequestStatus::Pending)
                .cloned()
                .collect::<Vec<_>>()
        })
    }

    fn list_all_request(&self) -> warp_common::Result<Vec<FriendRequest>> {
        let helper = self.friend_helper()?;
        let list = helper.list_requests()?;
        let ident = self.get_own_identity()?;
        let new_list = list
            .iter()
            .map(|(_, request)| request)
            .filter(|r| r.from == Pubkey::new(ident.public_key.to_bytes()))
            .map(fr_to_fr)
            .collect::<Vec<_>>();
        Ok(new_list)
    }

    fn remove_friend(&mut self, pubkey: PublicKey) -> warp_common::Result<()> {
        if self
            .get_identity(Identifier::PublicKey(pubkey.clone()))
            .is_err()
        {
            return Err(Error::Unimplemented);
        }

        if self.has_friend(pubkey.clone()).is_ok() {
            return Err(Error::Unimplemented);
        }

        let helper = self.friend_helper()?;

        helper.remove_friend(Pubkey::new(pubkey.to_bytes()))?;
        Ok(())
    }

    fn block_key(&mut self, _: PublicKey) -> warp_common::Result<()> {
        Err(Error::Unimplemented)
    }

    fn list_friends(&self) -> warp_common::Result<Vec<Identity>> {
        let mut identities = vec![];
        let list = self.list_all_request()?;
        for request in list
            .iter()
            .filter(|r| r.status == FriendRequestStatus::Accepted)
        {
            let identity = self.get_identity(Identifier::PublicKey(request.clone().to))?;
            identities.push(identity)
        }
        Ok(identities)
    }

    fn has_friend(&self, pubkey: PublicKey) -> warp_common::Result<()> {
        let helper = self.friend_helper()?;
        let request = helper.get_request(Pubkey::new(pubkey.to_bytes()))?;

        if request.status == Status::Accepted {
            return Ok(());
        }

        Err(Error::Unimplemented)
    }

    fn key_exchange(&self, _: Identity) -> warp_common::Result<Vec<u8>> {
        Err(Error::Unimplemented)
    }
}

fn fr_to_fr(fr: &helper::friends::FriendRequest) -> FriendRequest {
    let mut new_fr = FriendRequest::default();
    new_fr.status = match fr.status {
        Status::Uninitilized => FriendRequestStatus::Uninitialized,
        Status::Pending => FriendRequestStatus::Pending,
        Status::Accepted => FriendRequestStatus::Accepted,
        Status::Denied => FriendRequestStatus::Denied,
        Status::RemovedFriend => FriendRequestStatus::RequestRemoved,
        Status::RequestRemoved => FriendRequestStatus::RequestRemoved,
    };
    new_fr.from = PublicKey::from_bytes(&fr.from.to_bytes());
    new_fr.to = PublicKey::from_bytes(&fr.to.to_bytes());
    new_fr
}

fn user_to_identity(helper: &UserHelper, pubkey: Option<&[u8]>) -> anyhow::Result<Identity> {
    let (user, pubkey) = match pubkey {
        Some(pubkey) => {
            let pkey = Pubkey::new(pubkey);
            let user = helper.get_user(&pkey)?;
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
            identity.username = user.name;
        } else {
            match (
                split_data.get(0).ok_or(Error::Other).map(|s| s.to_string()),
                split_data.get(1).ok_or(Error::Other)?.parse(),
            ) {
                (Ok(name), Ok(code)) => {
                    identity.username = name;
                    identity.short_id = code;
                }
                _ => identity.username = user.name,
            };
        }
    } else {
        identity.username = user.name
    };

    identity.public_key = PublicKey::from_bytes(&pubkey.to_bytes());
    identity.status_message = Some(user.status);
    identity.graphics.profile_picture = user.photo_hash;
    identity.graphics.profile_banner = user.banner_image_hash;
    Ok(identity)
}
