pub mod friends;
pub mod groupchat;
pub mod server;
pub mod user;

//TODO: Have these configurable

use anchor_client::anchor_lang::{AccountDeserialize, Discriminator};
use anchor_client::solana_client::rpc_config::{RpcAccountInfoConfig, RpcProgramAccountsConfig};
use anchor_client::solana_client::rpc_filter::{Memcmp, MemcmpEncodedBytes, RpcFilterType};
use anchor_client::solana_sdk::account::Account;
use anchor_client::solana_sdk::bs58;
use anchor_client::solana_sdk::pubkey::Pubkey;
use anchor_client::{ClientError, Program};
use solana_account_decoder::UiAccountEncoding;
use std::iter::Map;
use std::str::FromStr;
use std::vec::IntoIter;

pub fn server_key() -> Pubkey {
    Pubkey::from_str("FGdpP9RSN3ZE8d1PXxiBXS8ThCsXdi342KmDwqSQ3ZBz").unwrap_or_default()
}

pub fn system_program_programid() -> Pubkey {
    Pubkey::from_str("11111111111111111111111111111111").unwrap_or_default()
}

//NOTE: This is copied as a temporary function due to the version of anchor being used not containing the functions

pub struct ProgramAccountsIterator<T> {
    inner: Map<IntoIter<(Pubkey, Account)>, AccountConverterFunction<T>>,
}

/// Function type that accepts solana accounts and returns deserialized anchor accounts
type AccountConverterFunction<T> = fn((Pubkey, Account)) -> Result<(Pubkey, T), ClientError>;

impl<T> Iterator for ProgramAccountsIterator<T> {
    type Item = Result<(Pubkey, T), ClientError>;

    fn next(&mut self) -> Option<Self::Item> {
        self.inner.next()
    }
}

pub fn accounts<T: AccountDeserialize + Discriminator>(
    program: &Program,
    filters: Vec<RpcFilterType>,
) -> Result<Vec<(Pubkey, T)>, ClientError> {
    accounts_lazy(program, filters)?.collect()
}

/// Returns all program accounts of the given type matching the given filters as an iterator
/// Deserialization is executed lazily
pub fn accounts_lazy<T: AccountDeserialize + Discriminator>(
    program: &Program,
    filters: Vec<RpcFilterType>,
) -> Result<ProgramAccountsIterator<T>, ClientError> {
    let account_type_filter = RpcFilterType::Memcmp(Memcmp {
        offset: 0,
        bytes: MemcmpEncodedBytes::Base58(bs58::encode(T::discriminator()).into_string()),
        encoding: None,
    });
    let config = RpcProgramAccountsConfig {
        filters: Some([vec![account_type_filter], filters].concat()),
        account_config: RpcAccountInfoConfig {
            encoding: Some(UiAccountEncoding::Base64),
            data_slice: None,
            commitment: None,
        },
        with_context: None,
    };
    Ok(ProgramAccountsIterator {
        inner: program
            .rpc()
            .get_program_accounts_with_config(&program.id(), config)?
            .into_iter()
            .map(|(key, account)| Ok((key, T::try_deserialize(&mut (&account.data as &[u8]))?))),
    })
}
