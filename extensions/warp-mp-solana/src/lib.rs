pub mod solana;

use anyhow::anyhow;
use warp::crypto::{rand::Rng, PublicKey};
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
use warp::{Extension, SingleHandle};

use crate::solana::helper::friends::{DirectFriendRequest, DirectStatus};
use crate::solana::helper::user::UserHelper;
use crate::solana::manager::SolanaManager;
use crate::solana::wallet::{PhraseType, SolanaWallet};
use crate::solana::{anchor_client::Cluster, helper};
use anchor_client::solana_sdk::pubkey::Pubkey;
use anchor_client::solana_sdk::signature::Keypair;

type Result<T> = std::result::Result<T, Error>;

pub struct SolanaAccount {
    pub endpoint: Cluster,
    pub contacts: Option<Vec<PublicKey>>,
    pub cache: Option<Arc<Mutex<Box<dyn PocketDimension>>>>,
    pub tesseract: Tesseract,
    pub hooks: Option<Hooks>,
}

impl Default for SolanaAccount {
    fn default() -> Self {
        Self {
            endpoint: Cluster::Devnet,
            contacts: None,
            cache: None,
            tesseract: Tesseract::default(),
            hooks: None,
        }
    }
}

impl SolanaAccount {
    pub fn new(endpoint: Cluster, tesseract: &Tesseract) -> Self {
        let tesseract = tesseract.clone();
        Self {
            tesseract,
            endpoint,
            ..Default::default()
        }
    }

    pub fn with_devnet(tesseract: &Tesseract) -> Self {
        Self::new(Cluster::Devnet, tesseract)
    }

    pub fn with_mainnet(tesseract: &Tesseract) -> Self {
        Self::new(Cluster::Mainnet, tesseract)
    }

    pub fn with_testnet(tesseract: &Tesseract) -> Self {
        Self::new(Cluster::Testnet, tesseract)
    }

    pub fn with_localnet(tesseract: &Tesseract) -> Self {
        Self::new(Cluster::Localnet, tesseract)
    }

    pub fn with_custom(url: &str, ws: &str, tesseract: &Tesseract) -> Self {
        Self::new(Cluster::Custom(url.to_string(), ws.to_string()), tesseract)
    }

    pub fn set_cache(&mut self, cache: Arc<Mutex<Box<dyn PocketDimension>>>) {
        self.cache = Some(cache);
    }

    pub fn set_hook(&mut self, hooks: &Hooks) {
        self.hooks = Some(hooks.clone())
    }

    pub fn insert_solana_wallet(&mut self, wallet: SolanaWallet) -> anyhow::Result<()> {
        let mut tesseract = self.get_tesseract()?;

        tesseract.set("mnemonic", &wallet.get_mnemonic_phrase()?)?;
        tesseract.set("privkey", wallet.get_keypair()?.to_base58_string().as_str())?;

        Ok(())
    }

    pub fn get_private_key(&self) -> anyhow::Result<Keypair> {
        let mut tesseract = self.get_tesseract()?;

        let private_key = match tesseract.retrieve("privkey") {
            Ok(key) => key,
            Err(e) => {
                if tesseract.exist("mnemonic") {
                    let mnemonic = tesseract.retrieve("mnemonic")?;
                    let wallet = SolanaWallet::restore_from_mnemonic(None, &mnemonic)?;
                    let kp = wallet.get_keypair()?.to_base58_string();
                    tesseract.set("privkey", kp.as_str())?;
                    kp
                } else {
                    return Err(anyhow!(e));
                }
            }
        };

        let keypair = Keypair::from_base58_string(private_key.as_str());
        Ok(keypair)
    }

    pub fn get_wallet(&self) -> Result<SolanaWallet> {
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

    pub fn friend_helper(&self) -> anyhow::Result<helper::friends::Friends> {
        let kp = self.get_private_key()?;
        let helper = helper::friends::Friends::new_with_cluster(self.endpoint.clone(), &kp);
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

    fn module(&self) -> Module {
        Module::Accounts
    }
}

impl SingleHandle for SolanaAccount {}

impl MultiPass for SolanaAccount {
    fn create_identity(&mut self, username: Option<&str>, _: Option<&str>) -> Result<PublicKey> {
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
            let object = DataObject::new(DataType::from(Module::Accounts), &identity)?;
            cache.add_data(DataType::from(Module::Accounts), &object)?;
        }
        if let Ok(hooks) = self.get_hooks() {
            let object = DataObject::new(DataType::Accounts, identity.clone())?;
            hooks.trigger("accounts::new_identity", &object);
        }
        Ok(identity.public_key())
    }

    fn get_identity(&self, id: Identifier) -> Result<Identity> {
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
                            return obj.payload::<Identity>();
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
                    query.r#where("public_key", &pkey)?;
                    if let Ok(list) = cache.get_data(DataType::from(Module::Accounts), Some(&query))
                    {
                        //get last
                        if !list.is_empty() {
                            let obj = list.last().unwrap();
                            return obj.payload::<Identity>();
                        }
                    }
                }
                user_to_identity(&helper, Some(pkey.as_ref()))?
            }
            (None, None, true) => user_to_identity(&helper, None)?,
            _ => return Err(Error::InvalidIdentifierCondition),
        };

        if let Ok(mut cache) = self.get_cache() {
            let mut query = QueryBuilder::default();
            query.r#where("public_key", &ident.public_key())?;
            if cache
                .has_data(DataType::from(Module::Accounts), &query)
                .is_err()
            {
                let object = DataObject::new(DataType::from(Module::Accounts), &ident)?;
                cache.add_data(DataType::from(Module::Accounts), &object)?;
            }
        }
        Ok(ident)
    }

    fn update_identity(&mut self, option: IdentityUpdate) -> Result<()> {
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

        if let Ok(hooks) = self.get_hooks() {
            let object = DataObject::new(DataType::Accounts, identity.clone())?;
            hooks.trigger("accounts::update_identity", &object);
        }

        Ok(())
    }

    fn decrypt_private_key(&self, _: Option<&str>) -> Result<Vec<u8>> {
        let keypair = self.get_private_key()?;
        Ok(keypair.to_bytes().to_vec())
    }

    fn refresh_cache(&mut self) -> Result<()> {
        self.get_cache()?.empty(DataType::from(self.module()))
    }
}

impl Friends for SolanaAccount {
    fn send_request(&mut self, pubkey: PublicKey) -> Result<()> {
        let ident = self.get_own_identity()?;

        if ident.public_key() == pubkey {
            return Err(Error::CannotSendSelfFriendRequest);
        }

        if self.get_identity(Identifier::from(pubkey.clone())).is_err() {
            return Err(Error::IdentityDoesntExist);
        }

        if self.has_friend(pubkey.clone()).is_ok() {
            return Err(Error::FriendExist);
        }

        let helper = self.friend_helper()?;

        helper.create_friend_request(Pubkey::new(pubkey.as_ref()), "")?;

        if let Ok(hooks) = self.get_hooks() {
            if let Some(request) = self
                .list_outgoing_request()?
                .iter()
                .filter(|request| request.to() == pubkey)
                .collect::<Vec<_>>()
                .first()
            {
                let object = DataObject::new(DataType::Accounts, request)?;
                hooks.trigger("accounts::send_friend_request", &object);
            }
        }
        Ok(())
    }

    fn accept_request(&mut self, pubkey: PublicKey) -> Result<()> {
        let ident = self.get_own_identity()?;

        if ident.public_key() == pubkey {
            return Err(Error::CannotAcceptSelfAsFriend);
        }

        if self.get_identity(Identifier::from(pubkey.clone())).is_err() {
            return Err(Error::IdentityDoesntExist);
        }

        if self.has_friend(pubkey.clone()).is_ok() {
            return Err(Error::FriendExist);
        }

        let helper = self.friend_helper()?;

        helper.accept_friend_request(Pubkey::new(pubkey.as_ref()), "")?;

        if let Ok(hooks) = self.get_hooks() {
            if let Some(request) = self
                .list_friends()?
                .iter()
                .filter(|pk| **pk == pubkey)
                .collect::<Vec<_>>()
                .first()
            {
                let object = DataObject::new(DataType::Accounts, request)?;
                hooks.trigger("accounts::accept_friend_request", &object);
            }
        }
        Ok(())
    }

    fn deny_request(&mut self, pubkey: PublicKey) -> Result<()> {
        let ident = self.get_own_identity()?;

        if ident.public_key() == pubkey {
            return Err(Error::CannotDenySelfAsFriend);
        }

        if self.get_identity(Identifier::from(pubkey.clone())).is_err() {
            return Err(Error::IdentityDoesntExist);
        }

        if self.has_friend(pubkey.clone()).is_ok() {
            return Err(Error::FriendExist);
        }

        let helper = self.friend_helper()?;

        helper.deny_friend_request(Pubkey::new(pubkey.as_ref()))?;
        if let Ok(hooks) = self.get_hooks() {
            if let Some(request) = self
                .list_all_request()?
                .iter()
                .filter(|request| request.from() == pubkey)
                .collect::<Vec<_>>()
                .first()
            {
                let object = DataObject::new(DataType::Accounts, request)?;
                hooks.trigger("accounts::deny_friend_request", &object);
            }
        }
        Ok(())
    }

    fn close_request(&mut self, pubkey: PublicKey) -> Result<()> {
        let ident = self.get_own_identity()?;

        if ident.public_key() == pubkey {
            return Err(Error::CannotUseSelfAsFriend);
        }

        if self.get_identity(Identifier::from(pubkey.clone())).is_err() {
            return Err(Error::IdentityDoesntExist);
        }

        if self.has_friend(pubkey.clone()).is_ok() {
            return Err(Error::FriendExist);
        }

        let helper = self.friend_helper()?;

        helper.close_friend_request(Pubkey::new(pubkey.as_ref()))?;
        if let Ok(hooks) = self.get_hooks() {
            if let Some(request) = self
                .list_all_request()?
                .iter()
                .filter(|request| request.from() == pubkey)
                .collect::<Vec<_>>()
                .first()
            {
                let object = DataObject::new(DataType::Accounts, request)?;
                hooks.trigger("accounts::close_friend_request", &object);
            }
        }
        Ok(())
    }

    fn list_incoming_request(&self) -> Result<Vec<FriendRequest>> {
        let helper = self.friend_helper()?;
        let list = helper
            .list_incoming_request()?
            .iter()
            .map(|(_, request)| request)
            .map(DirectFriendRequest::from)
            .map(fr_to_fr)
            .filter(|fr| fr.status() == FriendRequestStatus::Pending)
            .collect::<Vec<_>>();
        Ok(list)
    }

    fn list_outgoing_request(&self) -> Result<Vec<FriendRequest>> {
        let helper = self.friend_helper()?;
        let list = helper
            .list_outgoing_request()?
            .iter()
            .map(|(_, request)| request)
            .map(DirectFriendRequest::from)
            .map(fr_to_fr)
            .filter(|fr| fr.status() == FriendRequestStatus::Pending)
            .collect::<Vec<_>>();
        Ok(list)
    }

    fn list_all_request(&self) -> Result<Vec<FriendRequest>> {
        let helper = self.friend_helper()?;
        let list = helper.list_requests()?;
        let ident = self.get_own_identity()?;
        let new_list = list
            .iter()
            .map(|(_, request)| request)
            .map(DirectFriendRequest::from)
            .map(fr_to_fr)
            .filter(|fr| fr.from() == ident.public_key() || fr.to() == ident.public_key())
            .collect::<Vec<_>>();
        Ok(new_list)
    }

    fn remove_friend(&mut self, pubkey: PublicKey) -> Result<()> {
        if self.get_identity(Identifier::from(pubkey.clone())).is_err() {
            return Err(Error::CannotRemoveSelfAsFriend);
        }

        if self.has_friend(pubkey.clone()).is_err() {
            return Err(Error::FriendDoesntExist);
        }

        let helper = self.friend_helper()?;

        helper.remove_friend(Pubkey::new(pubkey.as_ref()))?;
        if let Ok(hooks) = self.get_hooks() {
            if let Some(request) = self
                .list_all_request()?
                .iter()
                .filter(|request| request.from() == pubkey || request.to() == pubkey)
                .collect::<Vec<_>>()
                .first()
            {
                let object = DataObject::new(DataType::Accounts, request)?;
                hooks.trigger("accounts::remove_friend", &object);
            }
        }
        Ok(())
    }

    fn list_friends(&self) -> Result<Vec<PublicKey>> {
        let mut identities = vec![];
        let list = self.list_all_request()?;
        let ident = self.get_own_identity()?;
        for request in list
            .iter()
            .filter(|r| r.status() == FriendRequestStatus::Accepted)
        {
            let identity = if request.to() != ident.public_key() {
                request.to()
            } else {
                request.from()
            };

            identities.push(identity)
        }
        Ok(identities)
    }

    fn has_friend(&self, pubkey: PublicKey) -> Result<()> {
        let helper = self.friend_helper()?;
        let request = helper
            .get_request(Pubkey::new(pubkey.as_ref()))
            .map(DirectFriendRequest::from)?;

        if request.status == DirectStatus::Accepted {
            return Ok(());
        }

        Err(Error::Any(anyhow!("Account is not friends")))
    }
}

fn fr_to_fr(fr: helper::friends::DirectFriendRequest) -> FriendRequest {
    let mut new_fr = FriendRequest::default();
    new_fr.set_status(match fr.status {
        DirectStatus::Uninitilized => FriendRequestStatus::Uninitialized,
        DirectStatus::Pending => FriendRequestStatus::Pending,
        DirectStatus::Accepted => FriendRequestStatus::Accepted,
        DirectStatus::Denied => FriendRequestStatus::Denied,
        DirectStatus::RemovedFriend => FriendRequestStatus::RequestRemoved,
        DirectStatus::RequestRemoved => FriendRequestStatus::RequestRemoved,
    });
    new_fr.set_from(PublicKey::from_bytes(&fr.from.to_bytes()));
    new_fr.set_to(PublicKey::from_bytes(&fr.to.to_bytes()));
    new_fr
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

    identity.set_public_key(PublicKey::from_bytes(&pubkey.to_bytes()));
    identity.set_status_message(Some(user.status));
    let mut graphics = Graphics::default();
    graphics.set_profile_banner(&user.banner_image_hash);
    graphics.set_profile_picture(&user.photo_hash);
    identity.set_graphics(graphics);
    Ok(identity)
}

pub mod ffi {
    use warp::multipass::MultiPassAdapter;
    use warp::pocket_dimension::PocketDimensionAdapter;
    use warp::sync::{Arc, Mutex};
    use warp::tesseract::Tesseract;

    use crate::SolanaAccount;

    #[allow(clippy::missing_safety_doc)]
    #[no_mangle]
    pub unsafe extern "C" fn multipass_mp_solana_new_with_devnet(
        pocketdimension: *const PocketDimensionAdapter,
        tesseract: *const Tesseract,
    ) -> *mut MultiPassAdapter {
        let tesseract = match tesseract.is_null() {
            false => {
                let tesseract = &*tesseract;
                tesseract.clone()
            }
            true => Tesseract::default(),
        };

        let mut account = SolanaAccount::with_devnet(&tesseract);
        match pocketdimension.is_null() {
            true => {}
            false => {
                let pd = &*pocketdimension;
                account.set_cache(pd.inner().clone());
            }
        }

        let mp = MultiPassAdapter::new(Arc::new(Mutex::new(Box::new(account))));
        Box::into_raw(Box::new(mp)) as *mut MultiPassAdapter
    }

    #[allow(clippy::missing_safety_doc)]
    #[no_mangle]
    pub unsafe extern "C" fn multipass_mp_solana_new_with_testnet(
        pocketdimension: *const PocketDimensionAdapter,
        tesseract: *const Tesseract,
    ) -> *mut MultiPassAdapter {
        let tesseract = match tesseract.is_null() {
            false => {
                let tesseract = &*tesseract;
                tesseract.clone()
            }
            true => Tesseract::default(),
        };
        let mut account = SolanaAccount::with_testnet(&tesseract);
        match pocketdimension.is_null() {
            true => {}
            false => {
                let pd = &*pocketdimension;
                account.set_cache(pd.inner().clone());
            }
        }

        let mp = MultiPassAdapter::new(Arc::new(Mutex::new(Box::new(account))));
        Box::into_raw(Box::new(mp)) as *mut MultiPassAdapter
    }

    #[allow(clippy::missing_safety_doc)]
    #[no_mangle]
    pub unsafe extern "C" fn multipass_mp_solana_new_with_mainnet(
        pocketdimension: *const PocketDimensionAdapter,
        tesseract: *const Tesseract,
    ) -> *mut MultiPassAdapter {
        let tesseract = match tesseract.is_null() {
            false => {
                let tesseract = &*(tesseract);
                tesseract.clone()
            }
            true => Tesseract::default(),
        };

        let mut account = SolanaAccount::with_mainnet(&tesseract);
        match pocketdimension.is_null() {
            true => {}
            false => {
                let pd = &*pocketdimension;
                account.set_cache(pd.inner().clone());
            }
        }

        let mp = MultiPassAdapter::new(Arc::new(Mutex::new(Box::new(account))));
        Box::into_raw(Box::new(mp)) as *mut MultiPassAdapter
    }
}
