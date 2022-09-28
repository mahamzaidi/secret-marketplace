#![cfg_attr(debug_assertions, allow(dead_code, unused_imports))]
#![allow(warnings, unused)]
use crate::rand::sha_256;
use cosmwasm_std::{
    log, to_binary, Api, Binary, BlockInfo, CanonicalAddr, CosmosMsg, Env, Extern, HandleResponse,
    HandleResult, HumanAddr, InitResponse, InitResult, Querier, QueryResult, ReadonlyStorage,
    StdError, StdResult, Storage, WasmMsg,
};
use cosmwasm_storage::{PrefixedStorage, ReadonlyPrefixedStorage};
use primitive_types::U256;
use std::collections::HashSet;

use secret_toolkit::{
    permit::{validate, Permit, RevokedPermits},
    utils::{pad_handle_result, pad_query_result},
};
use crate::inventory::{Inventory, InventoryIter};
use crate::msg::{
    AccessLevel, ContractStatus, HandleAnswer, HandleMsg, InitMsg, QueryAnswer, QueryMsg,
    ReceiverInfo, ResponseStatus::{Failure, Success}, Send, Transfer, ViewerInfo,
    Cw721OwnerOfResponse, Cw721Approval, List,
};
use crate::state::{
    json_may_load, json_save, load, may_load, remove, save, store_transfer, AuthList, Config,Permission, PermissionType, ReceiveRegistration, BLOCK_KEY, CONFIG_KEY, CREATOR_KEY, MY_ADDRESS_KEY, PREFIX_ALL_PERMISSIONS, PREFIX_AUTHLIST, PREFIX_INFOS, PREFIX_MAP_TO_ID, PREFIX_MAP_TO_INDEX, PREFIX_OWNER_PRIV, PREFIX_RECEIVERS, PREFIX_SELLERS, PREFIX_TX_IDS, PRNG_SEED_KEY, PREFIX_VIEW_KEY, PREFIX_PUB_META, PREFIX_PRIV_META, MINTERS_KEY, PREFIX_PRICE_KEY, PREFIX_AUCTION_KEY,
};

use crate::token::{Metadata, Token};
use crate::receiver::{receive_nft_msg, batch_receive_nft_msg, Snip721ReceiveMsg::{ReceiveNft, BatchReceiveNft}};
use crate::viewing_key::{ViewingKey, VIEWING_KEY_SIZE};

pub const MARKET_FEE: u128 = 300; //WRITE IN LOWEST DENOMINATION OF YOUR PREFERRED SNIP
pub const TOKEN_FEE: u128 = 5000;
pub const TOTAL_FEE: u128 = TOKEN_FEE - MARKET_FEE;
pub const BUYER: &str = "secret1y7anmvqjwnxqttrqjjmtqkj2fk4uh9ee77vs7z";
pub const SELLER: &str = "secret1f2xhf3ruydr7latjyypx6x08enattstqdertks";

/// pad handle responses and log attributes to blocks of 256 bytes to prevent leaking info based on
/// response size
pub const BLOCK_SIZE: usize = 256;
/// max number of token ids to keep in id list block
pub const ID_BLOCK_SIZE: u32 = 64;

////////////////////////////////////// Init ///////////////////////////////////////
/// Returns InitResult
///
/// Initializes the contract
///
/// # Arguments
///
/// * `deps` - mutable reference to Extern containing all the contract's external dependencies
/// * `env` - Env of contract's environment
/// * `msg` - InitMsg passed in with the instantiation message
pub fn init<S: Storage, A: Api, Q: Querier>(
    deps: &mut Extern<S, A, Q>,
    env: Env,
    msg: InitMsg,
) -> InitResult {
    let creator_raw = deps.api.canonical_address(&env.message.sender)?;
    save(&mut deps.storage, CREATOR_KEY, &creator_raw)?;
    save(
        &mut deps.storage,
        MY_ADDRESS_KEY,
        &deps.api.canonical_address(&env.contract.address)?,
    )?;
    let admin_raw = msg
        .admin
        .map(|a| deps.api.canonical_address(&a))
        .transpose()?
        .unwrap_or(creator_raw);

    let prng_seed: Vec<u8> = sha_256(base64::encode(msg.entropy).as_bytes()).to_vec();
    let view_key = base64::encode(&prng_seed);
    let init_config = msg.config.unwrap_or_default();
    let ps = &prng_seed;

    let mut config = Config {
        name: msg.name,
        admin: admin_raw.clone(),
        token_cnt: 0,
        tx_cnt: 0,
        mint_cnt: 0,
        status: ContractStatus::Normal.to_u8(),
        token_supply_is_public: init_config.public_token_supply.unwrap_or(true),
        owner_is_public: init_config.public_owner.unwrap_or(true),
        prng_seed: ps.to_vec(),
        entropy: String::default(),
        viewing_key: view_key,
        sealed_metadata_is_enabled: init_config.enable_sealed_metadata.unwrap_or(false),
        unwrap_to_private: init_config.unwrapped_metadata_is_private.unwrap_or(false),
        minter_may_update_metadata: init_config.minter_may_update_metadata.unwrap_or(false),
        owner_may_update_metadata: init_config.owner_may_update_metadata.unwrap_or(false),
        burn_is_enabled: init_config.enable_burn.unwrap_or(false),

    };


    save(&mut deps.storage, CONFIG_KEY, &config)?;
    save(&mut deps.storage, PRNG_SEED_KEY, &prng_seed)?;

    // TODO remove this after BlockInfo becomes available to queries
    save(&mut deps.storage, BLOCK_KEY, &env.block)?;

    Ok(InitResponse {
        messages: Vec::new(),
        log:vec![],
    })
}


// list of tokens sent from one previous owner
pub struct SendFrom {
    // the owner's address
    pub owner: HumanAddr,
    // the tokens that were sent
    pub token_ids: Vec<String>,
}

// permission type info
pub struct PermissionTypeInfo {
    // index for view owner permission
    pub view_owner_idx: usize,
    // index for view private metadata permission
    pub view_meta_idx: usize,
    // index for transfer permission
    pub transfer_idx: usize,
    // number of permission types
    pub num_types: usize,
}

// a receiver, their code hash, and whether they implement BatchReceiveNft
pub struct CacheReceiverInfo {
    // the contract address
    pub contract: CanonicalAddr,
    // the contract's registration info
    pub registration: ReceiveRegistration,
}

// an owner's inventory and the tokens they lost in this tx
pub struct InventoryUpdate {
    // owner's inventory
    pub inventory: Inventory,
    // the list of lost tokens
    pub remove: HashSet<u32>,
}

///////////////////////////////////// Handle //////////////////////////////////////
/// Returns HandleResult
///
/// # Arguments
///
/// * `deps` - mutable reference to Extern containing all the contract's external dependencies
/// * `env` - Env of contract's environment
/// * `msg` - HandleMsg passed in with the execute message
pub fn handle<S: Storage, A: Api, Q: Querier>(
    deps: &mut Extern<S, A, Q>,
    env: Env,
    msg: HandleMsg,
) -> HandleResult {
    // TODO remove this after BlockInfo becomes available to queries
    save(&mut deps.storage, BLOCK_KEY, &env.block)?;
    let mut config: Config = load(&deps.storage, CONFIG_KEY)?;

    let response = match msg {
        HandleMsg::RegisterReceiveNft {
            code_hash,
            also_implements_batch_receive_nft,
            ..
        } => register_receive_nft(
            deps,
            env,
            &config,
            ContractStatus::StopTransactions.to_u8(),
            code_hash,
            also_implements_batch_receive_nft,
        ),
        HandleMsg::SetContractStatus { level, .. } => {
            set_contract_status(deps, env, &mut config, level)
        },
        HandleMsg::TransferNft {
            recipient,
            token_id,
            memo,
            ..
        } => transfer_nft(
            deps,
            env,
            &mut config,
            ContractStatus::Normal.to_u8(),
            recipient,
            token_id,
            memo,
        ),
        HandleMsg::SendNft {
            contract,
            receiver_info,
            token_id,
            msg,
            memo,
            ..
        } => send_nft(
            deps,
            env,
            &mut config,
            ContractStatus::Normal.to_u8(),
            contract,
            receiver_info,
            token_id,
            msg,
            memo,
        ),
        HandleMsg::CreateViewingKey { entropy, .. } => create_key(
            deps,
            env,
            &config,
            ContractStatus::StopTransactions.to_u8(),
            &entropy,
        ),
        HandleMsg::SetViewingKey { key, .. } => set_key(
            deps,
            env,
            &config,
            ContractStatus::StopTransactions.to_u8(),
            key,
        ),
        HandleMsg::ChangeAdmin { address, .. } => change_admin(
            deps,
            env,
            &mut config,
            ContractStatus::StopTransactions.to_u8(),
            &address,
        ),
        HandleMsg::MakeOwnershipPrivate { .. } => 
            make_owner_private(deps, env, &config, ContractStatus::StopTransactions.to_u8()
        ),
        HandleMsg::ListNft{
            token_lists,
            sale_price,
            msg,
            memo,
        } => list_nft (
            deps, 
            env, 
            &mut config, 
            ContractStatus::Normal.to_u8(),
            token_lists, 
            sale_price,
            msg,
            memo,
        ),
        
    };
    pad_handle_result(response, BLOCK_SIZE)
}

pub fn list_nft<S: Storage, A: Api, Q: Querier>(
    deps: &mut Extern<S, A, Q>,
    env: Env,
    config: &mut Config,
    priority: u8,
    lists : Vec<List>,
    sale_price: String,
    msg: Option<Binary>,
    memo: Option<String>,
) -> HandleResult {
    check_status(config.status, priority)?;
    let num: u64 = sale_price.parse::<u64>().unwrap();
    if !num > 0 {
        return Err(StdError::generic_err(
            "Sale price must be greater than 0",
        ));
    }
    
    let mut inventories: Vec<Inventory> = Vec::new();
    let mut listed: Vec<String> = Vec::new();
    let trans_list = lists.clone();

    let binding = env.message.sender.to_string();
    let key: &[u8] = &binding.as_bytes();
    let val = &key;

    let sender_raw = deps.api.canonical_address(&env.message.sender)?;
    let recipient = env.contract.address.clone();
    
    // if(available_for_auction){
    //     let mut auction = PrefixedStorage::new(PREFIX_AUCTION_KEY, &mut deps.storage);
    //     save(&mut auction, val, &token_id)?;     
    // }
    for list in lists.into_iter() {
        //save price 
        let mut price = PrefixedStorage::new(PREFIX_PRICE_KEY, &mut deps.storage);
        save(&mut price, list.token_id.as_ref().unwrap().as_bytes(), &sale_price)?;
        let id = list.token_id.unwrap_or(format!("{}", config.token_cnt));
        // check if id already exists
        let mut map2idx = PrefixedStorage::new(PREFIX_MAP_TO_INDEX, &mut deps.storage);
        let may_exist: Option<u32> = may_load(&map2idx, id.as_bytes())?;
        if may_exist.is_some() {
            return Err(StdError::generic_err(format!(
                "Token ID {} is already in use",
                id
            )));
        }
        // increment token count
        config.token_cnt = config.token_cnt.checked_add(1).ok_or_else(|| {
            StdError::generic_err("Attempting to mint more tokens than the implementation limit")
        })?;
        // map new token id to its index
        save(&mut map2idx, id.as_bytes(), &config.token_cnt);

        let transferable = list.transferable.unwrap_or(true);    

        let token = Token {
            owner:deps.api.canonical_address(&env.message.sender)?,
            permissions: Vec::new(),
            unwrapped: !config.sealed_metadata_is_enabled,
            transferable,
        };

        // save new token info
        let token_key = config.token_cnt.to_le_bytes();
        let mut info_store = PrefixedStorage::new(PREFIX_INFOS, &mut deps.storage);
            json_save(&mut info_store, &token_key, &token)?;

        // add token to owner's list
        let inventory = if let Some(inv) = inventories.iter_mut().find(|i| i.owner == token.owner) {
            inv
        } else {
            let new_inv = Inventory::new(&deps.storage, token.owner.clone())?;
            inventories.push(new_inv);
            inventories.last_mut().ok_or_else(|| {
                StdError::generic_err("Just pushed an Inventory so this can not happen")
            })?
        };
        inventory.insert(&mut deps.storage, config.token_cnt, false)?;

        // map index to id
        let mut map2id = PrefixedStorage::new(PREFIX_MAP_TO_ID, &mut deps.storage);
        save(&mut map2id, &token_key, &id)?;

        if let Some(pub_meta) = list.public_metadata{
            enforce_metadata_field_exclusion(&pub_meta)?;
                let mut pub_store = PrefixedStorage::new(PREFIX_PUB_META, &mut deps.storage);
                save(&mut pub_store, &token_key, &pub_meta)?;
        }

        if let Some(priv_meta) = list.private_metadata {
            enforce_metadata_field_exclusion(&priv_meta)?;
            let mut priv_store = PrefixedStorage::new(PREFIX_PRIV_META, &mut deps.storage);
            save(&mut priv_store, &token_key, &priv_meta)?;
        }

    listed.push(id);

    }
    // save all the updated inventories
    for inventory in inventories.iter() {
        inventory.save(&mut deps.storage)?;
    }
    save(&mut deps.storage, CONFIG_KEY, &config)?;

    let messages = transfer_nft(
        deps, 
        env, 
        config,
        ContractStatus::Normal.to_u8(),
        recipient,
        trans_list[0].token_id.as_ref().map(String::as_str).unwrap().to_string(),
        memo,
    )?;
    let res = HandleResponse {
        messages: vec![],
        log: vec![],
        data: Some(to_binary(&HandleAnswer::ListNft { status: Success })?),
    };
    Ok(res)
}

/// Returns StdResult<()>
///
/// makes sure that Metadata does not have both `token_uri` and `extension`
///
/// # Arguments
///
/// * `metadata` - a reference to Metadata
fn enforce_metadata_field_exclusion(metadata: &Metadata) -> StdResult<()> {
    if metadata.token_uri.is_some() && metadata.extension.is_some() {
        return Err(StdError::generic_err(
            "Metadata can not have BOTH token_uri AND extension",
        ));
    }
    Ok(())
}

/// Returns StdResult<()>
///
/// sets new metadata
///
/// # Arguments
///
/// * `storage` - a mutable reference to the contract's storage
/// * `token` - a reference to the token whose metadata should be updated
/// * `idx` - the token identifier index
/// * `prefix` - storage prefix for the type of metadata being updated
/// * `metadata` - a reference to the new metadata
#[allow(clippy::too_many_arguments)]
fn set_metadata_impl<S: Storage>(
    storage: &mut S,
    token: &Token,
    idx: u32,
    prefix: &[u8],
    metadata: &Metadata,
) -> StdResult<()> {
    // do not allow the altering of sealed metadata
    if !token.unwrapped && prefix == PREFIX_PRIV_META {
        return Err(StdError::generic_err(
            "The private metadata of a sealed token can not be modified",
        ));
    }
    enforce_metadata_field_exclusion(metadata)?;
    let mut meta_store = PrefixedStorage::new(prefix, storage);
    save(&mut meta_store, &idx.to_le_bytes(), metadata)?;
    Ok(())
}
// pub fn sell_nft<S: Storage, A: Api, Q: Querier>(
//     deps: &mut Extern<S, A, Q>,
//     env: Env,
//     config: &mut Config,
//     priority: u8,
//     recipient: HumanAddr,
//     token_id: String,
//     memo: Option<String>,
// )

/// Returns HandleResult
///
/// makes an address' token ownership private
///
/// # Arguments
///
/// * `deps` - mutable reference to Extern containing all the contract's external dependencies
/// * `env` - Env of contract's environment
/// * `config` - a reference to the Config
/// * `priority` - u8 representation of highest status level this action is permitted at
pub fn make_owner_private<S: Storage, A: Api, Q: Querier>(
    deps: &mut Extern<S, A, Q>,
    env: Env,
    config: &Config,
    priority: u8,
) -> HandleResult {
    check_status(config.status, priority)?;
    let sender_raw = deps.api.canonical_address(&env.message.sender)?;
    // only need to do this if the contract has public ownership
    if config.owner_is_public {
        let mut priv_store = PrefixedStorage::new(PREFIX_OWNER_PRIV, &mut deps.storage);
        save(&mut priv_store, sender_raw.as_slice(), &false)?
    }
    Ok(HandleResponse {
        messages: vec![],
        log: vec![],
        data: Some(to_binary(&HandleAnswer::MakeOwnershipPrivate {
            status: Success,
        })?),
    })
}

/// Returns HandleResult
///
/// transfer a token
///
/// # Arguments
///
/// * `deps` - mutable reference to Extern containing all the contract's external dependencies
/// * `env` - Env of contract's environment
/// * `config` - a mutable reference to the Config
/// * `priority` - u8 representation of highest ContractStatus level this action is permitted
/// * `recipient` - the address receiving the token
/// * `token_id` - token id String of token to be transferred
/// * `memo` - optional memo for the mint tx
pub fn transfer_nft<S: Storage, A: Api, Q: Querier>(
    deps: &mut Extern<S, A, Q>,
    env: Env,
    config: &mut Config,
    priority: u8,
    recipient: HumanAddr,
    token_id: String,
    memo: Option<String>,
) -> HandleResult {
    check_status(config.status, priority)?;
    let sender_raw = deps.api.canonical_address(&env.message.sender)?;
    let transfers = Some(vec![Transfer {
        recipient,
        token_ids: vec![token_id],
        memo,
    }]);
    let _m = send_list(deps, &env, config, &sender_raw, transfers, None)?;

    let res = HandleResponse {
        messages: vec![],
        log: vec![],
        data: Some(to_binary(&HandleAnswer::TransferNft { status: Success })?),
    };
    Ok(res)
}



/// Returns HandleResult
///
/// sends a token to a contract, and calls that contract's ReceiveNft.  Will error if the
/// contract has not registered its ReceiveNft
///
/// # Arguments
///
/// * `deps` - mutable reference to Extern containing all the contract's external dependencies
/// * `env` - Env of contract's environment
/// * `config` - a mutable reference to the Config
/// * `priority` - u8 representation of highest ContractStatus level this action is permitted
/// * `contract` - the address of the contract receiving the token
/// * `receiver_info` - optional code hash and BatchReceiveNft implementation status of
///                     the recipient contract
/// * `token_id` - ID String of the token that was sent
/// * `msg` - optional msg used to control ReceiveNft logic
/// * `memo` - optional memo for the mint tx
#[allow(clippy::too_many_arguments)]
fn send_nft<S: Storage, A: Api, Q: Querier>(
    deps: &mut Extern<S, A, Q>,
    env: Env,
    config: &mut Config,
    priority: u8,
    contract: HumanAddr,
    receiver_info: Option<ReceiverInfo>,
    token_id: String,
    msg: Option<Binary>,
    memo: Option<String>,
) -> HandleResult {
    check_status(config.status, priority)?;
    let sender_raw = deps.api.canonical_address(&env.message.sender)?;
    let sends = Some(vec![Send {
        contract,
        receiver_info,
        token_ids: vec![token_id],
        msg,
        memo,
    }]);
    let messages = send_list(deps, &env, config, &sender_raw, None, sends)?;

    let res = HandleResponse {
        messages,
        log: vec![],
        data: Some(to_binary(&HandleAnswer::SendNft { status: Success })?),
    };
    Ok(res)
}

/// Returns HandleResult
///
/// creates a viewing key
///
/// # Arguments
///
/// * `deps` - mutable reference to Extern containing all the contract's external dependencies
/// * `env` - Env of contract's environment
/// * `config` - a reference to the Config
/// * `priority` - u8 representation of highest ContractStatus level this action is permitted
/// * `entropy` - string slice of the input String to be used as entropy in randomization
pub fn create_key<S: Storage, A: Api, Q: Querier>(
    deps: &mut Extern<S, A, Q>,
    env: Env,
    config: &Config,
    priority: u8,
    entropy: &str,
) -> HandleResult {
    check_status(config.status, priority)?;
    let prng_seed: Vec<u8> = load(&deps.storage, PRNG_SEED_KEY)?;
    let key = ViewingKey::new(&env, &prng_seed, entropy.as_ref());
    let message_sender = deps.api.canonical_address(&env.message.sender)?;
    let mut key_store = PrefixedStorage::new(PREFIX_VIEW_KEY, &mut deps.storage);
    save(&mut key_store, message_sender.as_slice(), &key.to_hashed())?;
    Ok(HandleResponse {
        messages: vec![],
        log: vec![],
        data: Some(to_binary(&HandleAnswer::ViewingKey {
            key: format!("{}", key),
        })?),
    })
}

/// Returns HandleResult
///
/// sets the viewing key to the input String
///
/// # Arguments
///
/// * `deps` - mutable reference to Extern containing all the contract's external dependencies
/// * `env` - Env of contract's environment
/// * `config` - a reference to the Config
/// * `priority` - u8 representation of highest ContractStatus level this action is permitted
/// * `key` - String to be used as the viewing key
pub fn set_key<S: Storage, A: Api, Q: Querier>(
    deps: &mut Extern<S, A, Q>,
    env: Env,
    config: &Config,
    priority: u8,
    key: String,
) -> HandleResult {
    check_status(config.status, priority)?;
    let vk = ViewingKey(key.clone());
    let message_sender = deps.api.canonical_address(&env.message.sender)?;
    let mut key_store = PrefixedStorage::new(PREFIX_VIEW_KEY, &mut deps.storage);
    save(&mut key_store, message_sender.as_slice(), &vk.to_hashed())?;
    Ok(HandleResponse {
        messages: vec![],
        log: vec![],
        data: Some(to_binary(&HandleAnswer::ViewingKey { key })?),
    })
}

/// Returns HandleResult
///
/// change the admin address
///
/// # Arguments
///
/// * `deps` - mutable reference to Extern containing all the contract's external dependencies
/// * `env` - Env of contract's environment
/// * `config` - a mutable reference to the Config
/// * `priority` - u8 representation of highest ContractStatus level this action is permitted
/// * `address` - new admin address
pub fn change_admin<S: Storage, A: Api, Q: Querier>(
    deps: &mut Extern<S, A, Q>,
    env: Env,
    config: &mut Config,
    priority: u8,
    address: &HumanAddr,
) -> HandleResult {
    check_status(config.status, priority)?;
    let sender_raw = deps.api.canonical_address(&env.message.sender)?;
    if config.admin != sender_raw {
        return Err(StdError::generic_err(
            "This is an admin command and can only be run from the admin address",
        ));
    }
    let new_admin = deps.api.canonical_address(address)?;
    if new_admin != config.admin {
        config.admin = new_admin;
        save(&mut deps.storage, CONFIG_KEY, &config)?;
    }
    Ok(HandleResponse {
        messages: vec![],
        log: vec![],
        data: Some(to_binary(&HandleAnswer::ChangeAdmin { status: Success })?),
    })
}

// pub fn try_receive<S: Storage, A: Api, Q: Querier>(
//     deps: &mut Extern<S, A, Q>,
//     sender: HumanAddr,
//     from: HumanAddr,
//     token_ids: &[String],
//     msg: Option<Binary>,
//     code_hash: String,
// ) -> HandleResult {
//     if token_ids.len() != 1 {
//         return Err(StdError::generic_err("You may only send one token!"));
//     }
//     let mut config: Config = load(&deps.storage, CONFIG_KEY)?;

//     // let sender_raw = deps.api.canonical_address(&env.message.sender)?;
//     // let contract_addr = deps.api.canonical_address(&env.contract);
//     //let amount_sent = env.message.sent_funds;

//     // increment token count
//     config.token_cnt = config.token_cnt.checked_add(1).ok_or_else(|| {
//         StdError::generic_err("Attempting to receive more tokens than the implementation limit")
//     })?;
//     save(&mut deps.storage, CONFIG_KEY, &config)?;

//     let mut token_store = PrefixedStorage::new(PREFIX_SELLERS, &mut deps.storage);
//     save(&mut token_store, SELLER.to_string().as_bytes(), &token_ids)?;

//     let res = HandleResponse {
//         messages: vec![],
//         log: vec![],
//         data: Some(to_binary(&HandleAnswer::BatchReceiveNft {
//             status: Success,
//         })?),
//     };
//     Ok(res)
// }

/// Returns HandleResult
///
/// registers a contract's ReceiveNft
///
/// # Arguments
///
/// * `deps` - mutable reference to Extern containing all the contract's external dependencies
/// * `env` - Env of contract's environment
/// * `config` - a reference to the Config
/// * `priority` - u8 representation of highest ContractStatus level this action is permitted
/// * `code_hash` - code hash String of the registering contract
/// * `impl_batch` - optionally true if the contract also implements BatchReceiveNft
pub fn register_receive_nft<S: Storage, A: Api, Q: Querier>(
    deps: &mut Extern<S, A, Q>,
    env: Env,
    config: &Config,
    priority: u8,
    code_hash: String,
    impl_batch: Option<bool>,
) -> HandleResult {
    check_status(config.status, priority)?;
    let sender_raw = deps.api.canonical_address(&env.message.sender)?;
    let regrec = ReceiveRegistration {
        code_hash,
        impl_batch: impl_batch.unwrap_or(false),
    };
    let mut store = PrefixedStorage::new(PREFIX_RECEIVERS, &mut deps.storage);
    save(&mut store, sender_raw.as_slice(), &regrec)?;

    let res = HandleResponse {
        messages: vec![],
        log: vec![],
        data: Some(to_binary(&HandleAnswer::RegisterReceiveNft {
            status: Success,
        })?),
    };
    Ok(res)
}

/// Returns HandleResult
///
/// set the contract status level
///
/// # Arguments
///
/// * `deps` - mutable reference to Extern containing all the contract's external dependencies
/// * `env` - Env of contract's environment
/// * `config` - a mutable reference to the Config
/// * `level` - new ContractStatus
pub fn set_contract_status<S: Storage, A: Api, Q: Querier>(
    deps: &mut Extern<S, A, Q>,
    env: Env,
    config: &mut Config,
    level: ContractStatus,
) -> HandleResult {
    let sender_raw = deps.api.canonical_address(&env.message.sender)?;
    if config.admin != sender_raw {
        return Err(StdError::generic_err(
            "This is an admin command and can only be run from the admin address",
        ));
    }
    let new_status = level.to_u8();
    if config.status != new_status {
        config.status = new_status;
        save(&mut deps.storage, CONFIG_KEY, &config)?;
    }
    Ok(HandleResponse {
        messages: vec![],
        log: vec![],
        data: Some(to_binary(&HandleAnswer::SetContractStatus {
            status: Success,
        })?),
    })
}

/// Returns StdResult<()>
///
/// update owners' inventories and AuthLists to reflect recent burns/transfers
///
/// # Arguments
///
/// * `storage` - a mutable reference to the contract's storage
/// * `updates` - a slice of an InventoryUpdate list to modify and store new inventories/AuthLists
/// * `num_perm_types` - the number of permission types
fn update_owner_inventory<S: Storage>(
    storage: &mut S,
    updates: &[InventoryUpdate],
    num_perm_types: usize,
) -> StdResult<()> {
    for update in updates {
        let owner_slice = update.inventory.owner.as_slice();
        // update the inventories
        update.inventory.save(storage)?;
        // update the AuthLists if tokens were lost
        if !update.remove.is_empty() {
            let mut auth_store = PrefixedStorage::new(PREFIX_AUTHLIST, storage);
            let may_list: Option<Vec<AuthList>> = may_load(&auth_store, owner_slice)?;
            if let Some(list) = may_list {
                let mut new_list = Vec::new();
                for mut auth in list.into_iter() {
                    for i in 0..num_perm_types {
                        auth.tokens[i].retain(|t| !update.remove.contains(t));
                    }
                    if !auth.tokens.iter().all(|u| u.is_empty()) {
                        new_list.push(auth)
                    }
                }
                if new_list.is_empty() {
                    remove(&mut auth_store, owner_slice);
                } else {
                    save(&mut auth_store, owner_slice, &new_list)?;
                }
            }
        }
    }
    Ok(())
}


/// Returns StdResult<()> that will error if the priority level of the action is not
/// equal to or greater than the current contract status level
///
/// # Arguments
///
/// * `contract_status` - u8 representation of the current contract status
/// * `priority` - u8 representing the highest status level this action may execute at
fn check_status(contract_status: u8, priority: u8) -> StdResult<()> {
    if priority < contract_status {
        return Err(StdError::generic_err(
            "The contract admin has temporarily disabled this action",
        ));
    }
    Ok(())
}


/// Returns StdResult<CanonicalAddr>
///
/// transfers a token, clears the token's permissions, and returns the previous owner's address
///
/// # Arguments
///
/// * `deps` - a mutable reference to Extern containing all the contract's external dependencies
/// * `block` - a reference to the current BlockInfo
/// * `config` - a mutable reference to the Config
/// * `sender` - a reference to the message sender address
/// * `token_id` - token id String of token being transferred
/// * `recipient` - the recipient's address
/// * `oper_for` - a mutable reference to a list of owners that gave the sender "all" permission
/// * `inv_updates` - a mutable reference to the list of token inventories to update
/// * `memo` - optional memo for the transfer tx
#[allow(clippy::too_many_arguments)]
fn transfer_impl<S: Storage, A: Api, Q: Querier>(
    deps: &mut Extern<S, A, Q>,
    block: &BlockInfo,
    config: &mut Config,
    sender: &CanonicalAddr,
    token_id: String,
    recipient: CanonicalAddr,
    oper_for: &mut Vec<CanonicalAddr>,
    inv_updates: &mut Vec<InventoryUpdate>,
    memo: Option<String>,
) -> StdResult<CanonicalAddr> {
    let (mut token, idx) = get_token_if_permitted(
        deps,
        block,
        &token_id,
        Some(sender),
        PermissionType::Transfer,
        oper_for,
        config,
    )?;
    if !token.transferable {
        return Err(StdError::generic_err(format!(
            "Token ID: {} is non-transferable",
            token_id
        )));
    }
    let old_owner = token.owner;
    // throw error if ownership would not change
    if old_owner == recipient {
        return Err(StdError::generic_err(format!(
            "Attempting to transfer token ID: {} to the address that already owns it",
            &token_id
        )));
    }
    token.owner = recipient.clone();
    token.permissions.clear();

    let update_addrs = vec![recipient.clone(), old_owner.clone()];
    // save updated token info
    let mut info_store = PrefixedStorage::new(PREFIX_INFOS, &mut deps.storage);
    json_save(&mut info_store, &idx.to_le_bytes(), &token)?;
    // log the inventory changes
    for addr in update_addrs.into_iter() {
        let inv_upd = if let Some(inv) = inv_updates.iter_mut().find(|i| i.inventory.owner == addr)
        {
            inv
        } else {
            let inventory = Inventory::new(&deps.storage, addr)?;
            let new_inv = InventoryUpdate {
                inventory,
                remove: HashSet::new(),
            };
            inv_updates.push(new_inv);
            inv_updates.last_mut().ok_or_else(|| {
                StdError::generic_err("Just pushed an InventoryUpdate so this can not happen")
            })?
        };
        // if updating the recipient's inventory
        if inv_upd.inventory.owner == recipient {
            inv_upd.inventory.insert(&mut deps.storage, idx, false)?;
        // else updating the old owner's inventory
        } else {
            inv_upd.inventory.remove(&mut deps.storage, idx, false)?;
            inv_upd.remove.insert(idx);
        }
    }

    let sndr = if old_owner == *sender {
        None
    } else {
        Some(sender.clone())
    };
    // store the tx
    store_transfer(
        &mut deps.storage,
        config,
        block,
        token_id,
        old_owner.clone(),
        sndr,
        recipient,
        memo,
    )?;
    Ok(old_owner)
}

/// Returns a StdResult<Vec<CosmosMsg>> list of ReceiveNft and BatchReceiveNft callacks that
/// should be done resulting from one Send
///
/// # Arguments
///
/// * `storage` - a reference to this contract's storage
/// * `contract_human` - a reference to the human address of the contract receiving the tokens
/// * `contract` - a reference to the canonical address of the contract receiving the tokens
/// * `receiver_info` - optional code hash and BatchReceiveNft implementation status of recipient contract
/// * `send_from_list` - list of SendFroms containing all the owners and their tokens being sent
/// * `msg` - a reference to the optional msg used to control ReceiveNft logic
/// * `sender` - a reference to the address that is sending the tokens
/// * `receivers` - a mutable reference the list of receiver contracts and their registration
///                 info

#[allow(clippy::too_many_arguments)]
fn receiver_callback_msgs<S: Storage, A: Api, Q: Querier>(
    deps: &mut Extern<S, A, Q>,
    env: &Env,
    contract_human: &HumanAddr,
    contract: &CanonicalAddr,
    receiver_info: Option<ReceiverInfo>,
    send_from_list: Vec<SendFrom>,
    msg: &Option<Binary>,
    sender: &HumanAddr,
    receivers: &mut Vec<CacheReceiverInfo>,
) -> StdResult<Vec<CosmosMsg>> {
    let (code_hash, impl_batch) = if let Some(supplied) = receiver_info {
        (
            supplied.recipient_code_hash,
            supplied.also_implements_batch_receive_nft.unwrap_or(false),
        )
    } else if let Some(receiver) = receivers.iter().find(|&r| r.contract == *contract) {
        (
            receiver.registration.code_hash.clone(),
            receiver.registration.impl_batch,
        )
    } else {
        let store = ReadonlyPrefixedStorage::new(PREFIX_RECEIVERS, &deps.storage);
        let registration: ReceiveRegistration =
            may_load(&store, contract.as_slice())?.unwrap_or(ReceiveRegistration {
                code_hash: String::new(),
                impl_batch: false,
            });
        let receiver = CacheReceiverInfo {
            contract: contract.clone(),
            registration: registration.clone(),
        };
        receivers.push(receiver);
        (registration.code_hash, registration.impl_batch)
    };
    let mut callbacks: Vec<CosmosMsg> = Vec::new();
    for send_from in send_from_list.into_iter() {
        // if BatchReceiveNft is implemented, use it
        if impl_batch {
            callbacks.push(batch_receive_nft_msg(
                deps,
                sender.clone(),
                send_from.owner,
                send_from.token_ids,
                msg.clone(),
                code_hash.clone(),
                contract_human.clone(),
            )?);
        //otherwise do a bunch of BatchReceiveNft
        } else {
            for token_id in send_from.token_ids.into_iter() {
                callbacks.push(receive_nft_msg(
                    deps,
                    send_from.owner.clone(),
                    token_id,
                    msg.clone(),
                    code_hash.clone(),
                    contract_human.clone(),
                )?);
            }
        }
    }
    Ok(callbacks)
}



/////////////////////////////////////// Query /////////////////////////////////////
/// Returns QueryResult
///
/// # Arguments
///
/// * `deps` - reference to Extern containing all the contract's external dependencies
/// * `msg` - QueryMsg passed in with the query call
pub fn query<S: Storage, A: Api, Q: Querier>(deps: &Extern<S, A, Q>, msg: QueryMsg) -> QueryResult {
    let response = match msg {
        QueryMsg::ContractInfo {} => query_contract_info(&deps.storage),
        QueryMsg::ContractCreator {} => query_contract_creator(deps),
        QueryMsg::ContractConfig {} => query_config(&deps.storage),
        QueryMsg::RegisteredCodeHash { contract } => query_code_hash(deps, &contract),
        QueryMsg::NumTokens { viewer } => query_num_tokens(deps, viewer, None),
        QueryMsg::AllTokens {
            viewer,
            start_after,
            limit,
        } => query_all_tokens(deps, viewer, start_after, limit, None),
        QueryMsg::OwnerOf {
            token_id,
            viewer,
            include_expired,
        } => query_owner_of(deps, &token_id, viewer, include_expired, None),
        QueryMsg::NftInfo { token_id } => query_nft_info(&deps.storage, &token_id),
        QueryMsg::PrivateMetadata { token_id, viewer } => {
            query_private_meta(deps, &token_id, viewer, None)
        }
        QueryMsg::AllNftInfo {
            token_id,
            viewer,
            include_expired,
        } => query_all_nft_info(deps, &token_id, viewer, include_expired, None),
        QueryMsg::Tokens {
            owner,
            viewer,
            viewing_key,
            start_after,
            limit,
        } => query_tokens(deps, &owner, viewer, viewing_key, start_after, limit, None),
        QueryMsg::NumTokensOfOwner {
            owner,
            viewer,
            viewing_key,
        } => query_num_owner_tokens(deps, &owner, viewer, viewing_key, None),
        QueryMsg::IsTransferable { token_id } => query_is_transferable(&deps.storage, &token_id),
        QueryMsg::ListedPrice { token_id } => query_listed_price(&deps.storage, &token_id),
    };
    pad_query_result(response, BLOCK_SIZE)
}


pub fn query_listed_price<S: ReadonlyStorage>(storage: &S, token_id: &str) -> QueryResult {
    let price_store = ReadonlyPrefixedStorage::new(PREFIX_PRICE_KEY, storage);
    let binding = token_id.to_string();
    let key: &[u8] = binding.as_bytes();
    let price: Option<String> = may_load(&price_store, key)?;
    let num: f64 = price.as_ref().map(String::as_str).unwrap().to_string().parse().unwrap();
    to_binary(&QueryAnswer::ListedPrice {
        price: num,
    })
}

/// Returns QueryResult displaying true if the token is transferable
///
/// # Arguments
///
/// * `storage` - a reference to the contract's storage
pub fn query_is_transferable<S: ReadonlyStorage>(storage: &S, token_id: &str) -> QueryResult {
    let config: Config = load(storage, CONFIG_KEY)?;
    let get_token_res = get_token(storage, token_id, None);
    match get_token_res {
        Err(err) => match err {
            // if the token id is not found, but token supply is private, just say
            // the token is transferable
            StdError::GenericErr { msg, .. }
                if !config.token_supply_is_public && msg.contains("Token ID") =>
            {
                to_binary(&QueryAnswer::IsTransferable {
                    token_is_transferable: true,
                })
            }
            _ => Err(err),
        },
        Ok((token, _idx)) => to_binary(&QueryAnswer::IsTransferable {
            token_is_transferable: token.transferable,
        }),
    }
}

/// Returns QueryResult displaying the number of tokens that the querier has permission to
/// view ownership and that belong to the specified address
///
/// # Arguments
///
/// * `deps` - a reference to Extern containing all the contract's external dependencies
/// * `owner` - a reference to the address whose tokens should be displayed
/// * `viewer` - optional address of the querier if different from the owner
/// * `viewing_key` - optional viewing key String
/// * `from_permit` - address derived from an Owner permit, if applicable
pub fn query_num_owner_tokens<S: Storage, A: Api, Q: Querier>(
    deps: &Extern<S, A, Q>,
    owner: &HumanAddr,
    viewer: Option<HumanAddr>,
    viewing_key: Option<String>,
    from_permit: Option<CanonicalAddr>,
) -> QueryResult {
    let owner_raw = deps.api.canonical_address(owner)?;
    // determine the querier
    let (is_owner, may_querier) = if let Some(pmt) = from_permit.as_ref() {
        // permit tells you who is querying, so also check if he is the owner
        (owner_raw == *pmt, from_permit)
    // no permit, so check if a key was provided and who it matches
    } else if let Some(key) = viewing_key {
        // if there is a viewer
        viewer
            // convert to canonical
            .map(|v| deps.api.canonical_address(&v))
            .transpose()?
            // only keep the viewer address if the viewing key matches
            .filter(|v| check_key(&deps.storage, v, key.clone()).is_ok())
            .map_or_else(
                // no viewer or key did not match
                || {
                    // check if the key matches the owner, and error if it fails this last chance
                    check_key(&deps.storage, &owner_raw, key)?;
                    Ok((true, Some(owner_raw.clone())))
                },
                // we know the querier is the viewer, so check if someone put the same address for both
                |v| Ok((v == owner_raw, Some(v))),
            )?
    // no permit, no viewing key, so querier is unknown
    } else {
        (false, None)
    };

    // get list of owner's tokens
    let own_inv = Inventory::new(&deps.storage, owner_raw)?;
    let owner_slice = own_inv.owner.as_slice();

    // if querier is different than the owner, check if ownership is public
    let mut known_pass = if !is_owner {
        let config: Config = load(&deps.storage, CONFIG_KEY)?;
        let own_priv_store = ReadonlyPrefixedStorage::new(PREFIX_OWNER_PRIV, &deps.storage);
        let pass: bool = may_load(&own_priv_store, owner_slice)?.unwrap_or(config.owner_is_public);
        pass
    } else {
        true
    };
    // TODO remove this when BlockInfo becomes available to queries
    let block = if !known_pass {
        let b: BlockInfo = may_load(&deps.storage, BLOCK_KEY)?.unwrap_or_else(|| BlockInfo {
            height: 1,
            time: 1,
            chain_id: "not used".to_string(),
        });
        b
    } else {
        BlockInfo {
            height: 1,
            time: 1,
            chain_id: "not used".to_string(),
        }
    };
    let exp_idx = PermissionType::ViewOwner.to_usize();
    let global_raw = CanonicalAddr(Binary::from(b"public"));
    let (sender, only_public) = if let Some(sdr) = may_querier.as_ref() {
        (sdr, false)
    } else {
        (&global_raw, true)
    };
    let mut found_one = only_public;
    if !known_pass {
        // check if the ownership has been made public or the sender has ALL permission.
        let all_store = ReadonlyPrefixedStorage::new(PREFIX_ALL_PERMISSIONS, &deps.storage);
        let may_list: Option<Vec<Permission>> = json_may_load(&all_store, owner_slice)?;
        if let Some(list) = may_list {
            for perm in &list {
                if perm.address == *sender || perm.address == global_raw {
                    if let Some(exp) = perm.expirations[exp_idx] {
                        if !exp.is_expired(&block) {
                            known_pass = true;
                            break;
                        }
                    }
                    // we can quit if we found both the sender and the global (or if only searching for public)
                    if found_one {
                        break;
                    } else {
                        found_one = true;
                    }
                }
            }
        }
    }
    // if it is either the owner, ownership is public, or the querier has inventory-wide view owner permission,
    // let them see the full count
    if known_pass {
        return to_binary(&QueryAnswer::NumTokens {
            count: own_inv.info.count,
        });
    }

    // get the list of tokens that might have viewable ownership for this querier
    let mut token_idxs: HashSet<u32> = HashSet::new();
    found_one = only_public;
    let auth_store = ReadonlyPrefixedStorage::new(PREFIX_AUTHLIST, &deps.storage);
    let auth_list: Vec<AuthList> = may_load(&auth_store, owner_slice)?.unwrap_or_default();
    for auth in auth_list.iter() {
        if auth.address == *sender || auth.address == global_raw {
            token_idxs.extend(auth.tokens[exp_idx].iter());
            if found_one {
                break;
            } else {
                found_one = true;
            }
        }
    }
    // check if the the token permissions have expired, and if not include it in the count
    let info_store = ReadonlyPrefixedStorage::new(PREFIX_INFOS, &deps.storage);
    let mut count = 0u32;
    for idx in token_idxs.into_iter() {
        if let Some(token) = json_may_load::<Token, _>(&info_store, &idx.to_le_bytes())? {
            found_one = only_public;
            for perm in token.permissions.iter() {
                if perm.address == *sender || perm.address == global_raw {
                    if let Some(exp) = perm.expirations[exp_idx] {
                        if !exp.is_expired(&block) {
                            count += 1;
                            break;
                        }
                    }
                    // we can quit if we found both the sender and the global (or if only searching for public)
                    if found_one {
                        break;
                    } else {
                        found_one = true;
                    }
                }
            }
        }
    }

    to_binary(&QueryAnswer::NumTokens { count })
}

/// Returns QueryResult displaying an optionally paginated list of all tokens belonging to
/// the owner address.  It will only display the tokens that the querier has view_owner
/// approval
///
/// # Arguments
///
/// * `deps` - a reference to Extern containing all the contract's external dependencies
/// * `owner` - a reference to the address whose tokens should be displayed
/// * `viewer` - optional address of the querier if different from the owner
/// * `viewing_key` - optional viewing key String
/// * `start_after` - optionally only display token ids that come after this String in
///                   lexicographical order
/// * `limit` - optional max number of tokens to display
/// * `from_permit` - address derived from an Owner permit, if applicable
pub fn query_tokens<S: Storage, A: Api, Q: Querier>(
    deps: &Extern<S, A, Q>,
    owner: &HumanAddr,
    viewer: Option<HumanAddr>,
    viewing_key: Option<String>,
    start_after: Option<String>,
    limit: Option<u32>,
    from_permit: Option<CanonicalAddr>,
) -> QueryResult {
    let owner_raw = deps.api.canonical_address(owner)?;
    let cut_off = limit.unwrap_or(30);
    // determine the querier
    let (is_owner, may_querier) = if let Some(pmt) = from_permit.as_ref() {
        // permit tells you who is querying, so also check if he is the owner
        (owner_raw == *pmt, from_permit)
    // no permit, so check if a key was provided and who it matches
    } else if let Some(key) = viewing_key {
        // if there is a viewer
        viewer
            // convert to canonical
            .map(|v| deps.api.canonical_address(&v))
            .transpose()?
            // only keep the viewer address if the viewing key matches
            .filter(|v| check_key(&deps.storage, v, key.clone()).is_ok())
            .map_or_else(
                // no viewer or key did not match
                || {
                    // check if the key matches the owner, and error if it fails this last chance
                    check_key(&deps.storage, &owner_raw, key)?;
                    Ok((true, Some(owner_raw.clone())))
                },
                // we know the querier is the viewer, so check if someone put the same address for both
                |v| Ok((v == owner_raw, Some(v))),
            )?
    // no permit, no viewing key, so querier is unknown
    } else {
        (false, None)
    };
    // exit early if the limit is 0
    if cut_off == 0 {
        return to_binary(&QueryAnswer::TokenList { tokens: Vec::new() });
    }
    // get list of owner's tokens
    let own_inv = Inventory::new(&deps.storage, owner_raw)?;
    let owner_slice = own_inv.owner.as_slice();

    let querier = may_querier.as_ref();
    // if querier is different than the owner, check if ownership is public
    let mut may_config: Option<Config> = None;
    let mut known_pass = if !is_owner {
        let config: Config = load(&deps.storage, CONFIG_KEY)?;
        let own_priv_store = ReadonlyPrefixedStorage::new(PREFIX_OWNER_PRIV, &deps.storage);
        let pass: bool = may_load(&own_priv_store, owner_slice)?.unwrap_or(config.owner_is_public);
        may_config = Some(config);
        pass
    } else {
        true
    };
    // TODO remove this when BlockInfo becomes available to queries
    let block = if !known_pass {
        let b: BlockInfo = may_load(&deps.storage, BLOCK_KEY)?.unwrap_or_else(|| BlockInfo {
            height: 1,
            time: 1,
            chain_id: "not used".to_string(),
        });
        b
    } else {
        BlockInfo {
            height: 1,
            time: 1,
            chain_id: "not used".to_string(),
        }
    };
    let exp_idx = PermissionType::ViewOwner.to_usize();
    let info_store = ReadonlyPrefixedStorage::new(PREFIX_INFOS, &deps.storage);
    let mut list_it: bool;
    let mut oper_for: Vec<CanonicalAddr> = Vec::new();
    let mut tokens: Vec<String> = Vec::new();
    let map2id = ReadonlyPrefixedStorage::new(PREFIX_MAP_TO_ID, &deps.storage);
    let mut inv_iter = if let Some(after) = start_after.as_ref() {
        // load the config if we haven't already
        let config = may_config.map_or_else(|| load::<Config, _>(&deps.storage, CONFIG_KEY), Ok)?;
        // if the querier is allowed to view all of the owner's tokens, let them know if the token
        // does not belong to the owner
        let inv_err = format!("Token ID: {} is not in the specified inventory", after);
        // or tell any other viewer that they are not authorized
        let unauth_err = format!(
            "You are not authorized to perform this action on token {}",
            after
        );
        let public_err = format!("Token ID: {} not found", after);
        // if token supply is public let them know if the token id does not exist
        let not_found_err = if config.token_supply_is_public {
            &public_err
        } else if known_pass {
            &inv_err
        } else {
            &unauth_err
        };
        let map2idx = ReadonlyPrefixedStorage::new(PREFIX_MAP_TO_INDEX, &deps.storage);
        let idx: u32 = may_load(&map2idx, after.as_bytes())?
            .ok_or_else(|| StdError::generic_err(not_found_err))?;
        // make sure querier is allowed to know if the supplied token belongs to owner
        if !known_pass {
            let token: Token = json_may_load(&info_store, &idx.to_le_bytes())?
                .ok_or_else(|| StdError::generic_err("Token info storage is corrupt"))?;
            // if the specified token belongs to the specified owner, save if the querier is an operator
            let mut may_oper_vec = if own_inv.owner == token.owner {
                None
            } else {
                Some(Vec::new())
            };
            check_perm_core(
                deps,
                &block,
                &token,
                after,
                querier,
                token.owner.as_slice(),
                exp_idx,
                may_oper_vec.as_mut().unwrap_or(&mut oper_for),
                &unauth_err,
            )?;
            // if querier is found to have ALL permission for the specified owner, no need to check permission ever again
            if !oper_for.is_empty() {
                known_pass = true;
            }
        }
        InventoryIter::start_after(&deps.storage, &own_inv, idx, &inv_err)?
    } else {
        InventoryIter::new(&own_inv)
    };
    let mut count = 0u32;
    while let Some(idx) = inv_iter.next(&deps.storage)? {
        if let Some(id) = may_load::<String, _>(&map2id, &idx.to_le_bytes())? {
            list_it = known_pass;
            // only check permissions if not public or owner
            if !known_pass {
                if let Some(token) = json_may_load::<Token, _>(&info_store, &idx.to_le_bytes())? {
                    list_it = check_perm_core(
                        deps,
                        &block,
                        &token,
                        &id,
                        querier,
                        owner_slice,
                        exp_idx,
                        &mut oper_for,
                        "",
                    )
                    .is_ok();
                    // if querier is found to have ALL permission, no need to check permission ever again
                    if !oper_for.is_empty() {
                        known_pass = true;
                    }
                }
            }
            if list_it {
                tokens.push(id);
                // it'll hit the gas ceiling before overflowing the count
                count += 1;
                // exit if we hit the limit
                if count >= cut_off {
                    break;
                }
            }
        }
    }
    to_binary(&QueryAnswer::TokenList { tokens })
}

/// Returns QueryResult displaying the list of tokens that the contract controls
///
/// # Arguments
///
/// * `deps` - a reference to Extern containing all the contract's external dependencies
/// * `viewer` - optional address and key making an authenticated query request
/// * `start_after` - optionally only display token ids that come after this one
/// * `limit` - optional max number of tokens to display
/// * `from_permit` - address derived from an Owner permit, if applicable
pub fn query_all_tokens<S: Storage, A: Api, Q: Querier>(
    deps: &Extern<S, A, Q>,
    viewer: Option<ViewerInfo>,
    start_after: Option<String>,
    limit: Option<u32>,
    from_permit: Option<CanonicalAddr>,
) -> QueryResult {
    // authenticate permission to view token supply
    check_view_supply(deps, viewer, from_permit)?;
    let mut i = start_after.map_or_else(
        || Ok(0),
        |id| {
            let map2idx = ReadonlyPrefixedStorage::new(PREFIX_MAP_TO_INDEX, &deps.storage);
            let idx: u32 = may_load(&map2idx, id.as_bytes())?
                .ok_or_else(|| StdError::generic_err(format!("Token ID: {} not found", id)))?;
            idx.checked_add(1).ok_or_else(|| {
                StdError::generic_err("This token was the last one the contract could mint")
            })
        },
    )?;
    let cut_off = limit.unwrap_or(300);
    let config: Config = load(&deps.storage, CONFIG_KEY)?;
    let mut tokens = Vec::new();
    let mut count = 0u32;
    let map2id = ReadonlyPrefixedStorage::new(PREFIX_MAP_TO_ID, &deps.storage);
    while count < cut_off {
        if let Some(id) = may_load::<String, _>(&map2id, &i.to_le_bytes())? {
            tokens.push(id);
            // will hit gas ceiling before the count overflows
            count += 1;
        }
        // i can't overflow if it was less than a u32
        i += 1;
    }
    to_binary(&QueryAnswer::TokenList { tokens })
}

/// Returns QueryResult displaying the owner of the input token if the requester is authorized
/// to view it and the transfer approvals on this token if the owner is querying
///
/// # Arguments
///
/// * `deps` - a reference to Extern containing all the contract's external dependencies
/// * `token_id` - string slice of the token id
/// * `viewer` - optional address and key making an authenticated query request
/// * `include_expired` - optionally true if the Approval lists should include expired Approvals
/// * `from_permit` - address derived from an Owner permit, if applicable
pub fn query_owner_of<S: Storage, A: Api, Q: Querier>(
    deps: &Extern<S, A, Q>,
    token_id: &str,
    viewer: Option<ViewerInfo>,
    include_expired: Option<bool>,
    from_permit: Option<CanonicalAddr>,
) -> QueryResult {
    let (may_owner, approvals, _idx) =
        process_cw721_owner_of(deps, token_id, viewer, include_expired, from_permit)?;
    if let Some(owner) = may_owner {
        return to_binary(&QueryAnswer::OwnerOf { owner, approvals });
    }
    Err(StdError::generic_err(format!(
        "You are not authorized to view the owner of token {}",
        token_id
    )))
}

/// Returns QueryResult displaying the public metadata of a token
///
/// # Arguments
///
/// * `storage` - a reference to the contract's storage
/// * `token_id` - string slice of the token id
pub fn query_nft_info<S: ReadonlyStorage>(storage: &S, token_id: &str) -> QueryResult {
    let map2idx = ReadonlyPrefixedStorage::new(PREFIX_MAP_TO_INDEX, storage);
    let may_idx: Option<u32> = may_load(&map2idx, token_id.as_bytes())?;
    // if token id was found
    if let Some(idx) = may_idx {
        let meta_store = ReadonlyPrefixedStorage::new(PREFIX_PUB_META, storage);
        let meta: Metadata = may_load(&meta_store, &idx.to_le_bytes())?.unwrap_or(Metadata {
            token_uri: None,
            extension: None,
        });
        return to_binary(&QueryAnswer::NftInfo {
            token_uri: meta.token_uri,
            extension: meta.extension,
        });
    }
    let config: Config = load(storage, CONFIG_KEY)?;
    // token id wasn't found
    // if the token supply is public, let them know the token does not exist
    if config.token_supply_is_public {
        return Err(StdError::generic_err(format!(
            "Token ID: {} not found",
            token_id
        )));
    }
    // otherwise, just return empty metadata
    to_binary(&QueryAnswer::NftInfo {
        token_uri: None,
        extension: None,
    })
}

/// Returns QueryResult displaying the private metadata of a token if permitted to
/// view it
///
/// # Arguments
///
/// * `deps` - a reference to Extern containing all the contract's external dependencies
/// * `token_id` - string slice of the token id
/// * `viewer` - optional address and key making an authenticated query request
/// * `from_permit` - address derived from an Owner permit, if applicable
pub fn query_private_meta<S: Storage, A: Api, Q: Querier>(
    deps: &Extern<S, A, Q>,
    token_id: &str,
    viewer: Option<ViewerInfo>,
    from_permit: Option<CanonicalAddr>,
) -> QueryResult {
    let prep_info = query_token_prep(deps, token_id, viewer, from_permit)?;
    check_perm_core(
        deps,
        &prep_info.block,
        &prep_info.token,
        token_id,
        prep_info.viewer_raw.as_ref(),
        prep_info.token.owner.as_slice(),
        PermissionType::ViewMetadata.to_usize(),
        &mut Vec::new(),
        &prep_info.err_msg,
    )?;
    // don't display if private metadata is sealed
    if !prep_info.token.unwrapped {
        return Err(StdError::generic_err(
            "Sealed metadata must be unwrapped by calling Reveal before it can be viewed",
        ));
    }
    let meta_store = ReadonlyPrefixedStorage::new(PREFIX_PRIV_META, &deps.storage);
    let meta: Metadata = may_load(&meta_store, &prep_info.idx.to_le_bytes())?.unwrap_or(Metadata {
        token_uri: None,
        extension: None,
    });
    to_binary(&QueryAnswer::PrivateMetadata {
        token_uri: meta.token_uri,
        extension: meta.extension,
    })
}

/// Returns QueryResult displaying response of both the OwnerOf and NftInfo queries
///
/// # Arguments
///
/// * `deps` - a reference to Extern containing all the contract's external dependencies
/// * `token_id` - string slice of the token id
/// * `viewer` - optional address and key making an authenticated query request
/// * `include_expired` - optionally true if the Approval lists should include expired Approvals
/// * `from_permit` - address derived from an Owner permit, if applicable
pub fn query_all_nft_info<S: Storage, A: Api, Q: Querier>(
    deps: &Extern<S, A, Q>,
    token_id: &str,
    viewer: Option<ViewerInfo>,
    include_expired: Option<bool>,
    from_permit: Option<CanonicalAddr>,
) -> QueryResult {
    let (owner, approvals, idx) =
        process_cw721_owner_of(deps, token_id, viewer, include_expired, from_permit)?;
    let meta_store = ReadonlyPrefixedStorage::new(PREFIX_PUB_META, &deps.storage);
    let info: Option<Metadata> = may_load(&meta_store, &idx.to_le_bytes())?;
    let access = Cw721OwnerOfResponse { owner, approvals };
    to_binary(&QueryAnswer::AllNftInfo { access, info })
}


/// Returns QueryResult displaying the number of tokens the contract controls
///
/// # Arguments
///
/// * `deps` - a reference to Extern containing all the contract's external dependencies
/// * `viewer` - optional address and key making an authenticated query request
/// * `from_permit` - address derived from an Owner permit, if applicable
pub fn query_num_tokens<S: Storage, A: Api, Q: Querier>(
    deps: &Extern<S, A, Q>,
    viewer: Option<ViewerInfo>,
    from_permit: Option<CanonicalAddr>,
) -> QueryResult {
    // authenticate permission to view token supply
    check_view_supply(deps, viewer, from_permit)?;
    let config: Config = load(&deps.storage, CONFIG_KEY)?;
    to_binary(&QueryAnswer::NumTokens {
        count: config.token_cnt,
    })
}

/// Returns QueryResult displaying the contract's creator
///
/// # Arguments
///
/// * `deps` - a reference to Extern containing all the contract's external dependencies
pub fn query_contract_creator<S: Storage, A: Api, Q: Querier>(
    deps: &Extern<S, A, Q>,
) -> QueryResult {
    let creator_raw: CanonicalAddr = load(&deps.storage, CREATOR_KEY)?;
    to_binary(&QueryAnswer::ContractCreator {
        creator: Some(deps.api.human_address(&creator_raw)?),
    })
}

/// Returns QueryResult displaying the contract's name and symbol
///
/// # Arguments
///
/// * `storage` - a reference to the contract's storage
pub fn query_contract_info<S: ReadonlyStorage>(storage: &S) -> QueryResult {
    let config: Config = load(storage, CONFIG_KEY)?;

    to_binary(&QueryAnswer::ContractInfo { name: config.name })
}

/// Returns QueryResult displaying the contract's configuration
///
/// # Arguments
///
/// * `storage` - a reference to the contract's storage
pub fn query_config<S: ReadonlyStorage>(storage: &S) -> QueryResult {
    let config: Config = load(storage, CONFIG_KEY)?;

    to_binary(&QueryAnswer::ContractConfig {
        token_supply_is_public: config.token_supply_is_public,
        owner_is_public: config.owner_is_public,
    })
}

/// Returns QueryResult displaying the registered code hash of the specified contract if
/// it has registered and whether the contract implements BatchReceiveNft
///
/// # Arguments
///
/// * `deps` - a reference to Extern containing all the contract's external dependencies
/// * `contract` - a reference to the contract's address whose code hash is being requested
pub fn query_code_hash<S: Storage, A: Api, Q: Querier>(
    deps: &Extern<S, A, Q>,
    contract: &HumanAddr,
) -> QueryResult {
    let contract_raw = deps.api.canonical_address(contract)?;
    let store = ReadonlyPrefixedStorage::new(PREFIX_RECEIVERS, &deps.storage);
    let may_reg_rec: Option<ReceiveRegistration> = may_load(&store, contract_raw.as_slice())?;
    if let Some(reg_rec) = may_reg_rec {
        return to_binary(&QueryAnswer::RegisteredCodeHash {
            code_hash: Some(reg_rec.code_hash),
            also_implements_batch_receive_nft: reg_rec.impl_batch,
        });
    }
    to_binary(&QueryAnswer::RegisteredCodeHash {
        code_hash: None,
        also_implements_batch_receive_nft: false,
    })
}


/// Returns StdResult<Vec<CosmosMsg>>
///
/// transfer or sends a list of tokens and returns a list of ReceiveNft callbacks if applicable
///
/// # Arguments
///
/// * `deps` - a mutable reference to Extern containing all the contract's external dependencies
/// * `env` - a reference to the Env of the contract's environment
/// * `config` - a mutable reference to the Config
/// * `sender` - a reference to the message sender address
/// * `transfers` - optional list of transfers to perform
/// * `sends` - optional list of sends to perform
fn send_list<S: Storage, A: Api, Q: Querier>(
    deps: &mut Extern<S, A, Q>,
    env: &Env,
    config: &mut Config,
    sender: &CanonicalAddr,
    transfers: Option<Vec<Transfer>>,
    sends: Option<Vec<Send>>,
) -> StdResult<Vec<CosmosMsg>> {
    let mut messages: Vec<CosmosMsg> = Vec::new();
    let mut oper_for: Vec<CanonicalAddr> = Vec::new();
    let mut inv_updates: Vec<InventoryUpdate> = Vec::new();
    let num_perm_types = PermissionType::ViewOwner.num_types();
    if let Some(xfers) = transfers {
        for xfer in xfers.into_iter() {
            let recipient_raw = deps.api.canonical_address(&xfer.recipient)?;
            for token_id in xfer.token_ids.into_iter() {
                let _o = transfer_impl(
                    deps,
                    &env.block,
                    config,
                    sender,
                    token_id,
                    recipient_raw.clone(),
                    &mut oper_for,
                    &mut inv_updates,
                    xfer.memo.clone(),
                )?;
            }
        }
    } else if let Some(snds) = sends {
        let mut receivers = Vec::new();
        for send in snds.into_iter() {
            let contract_raw = deps.api.canonical_address(&send.contract)?;
            let mut send_from_list: Vec<SendFrom> = Vec::new();
            for token_id in send.token_ids.into_iter() {
                let owner_raw = transfer_impl(
                    deps,
                    &env.block,
                    config,
                    sender,
                    token_id.clone(),
                    contract_raw.clone(),
                    &mut oper_for,
                    &mut inv_updates,
                    send.memo.clone(),
                )?;
                // compile list of all tokens being sent from each owner in this Send
                let owner = deps.api.human_address(&owner_raw)?;
                if let Some(sd_fm) = send_from_list.iter_mut().find(|s| s.owner == owner) {
                    sd_fm.token_ids.push(token_id.clone());
                } else {
                    let new_sd_fm = SendFrom {
                        owner,
                        token_ids: vec![token_id.clone()],
                    };
                    send_from_list.push(new_sd_fm);
                }
            }
            // get BatchReceiveNft and ReceiveNft msgs for all the tokens sent in this Send
            messages.extend(receiver_callback_msgs(
                deps,
                &env,
                &send.contract,
                &contract_raw,
                send.receiver_info,
                send_from_list,
                &send.msg,
                &env.message.sender,
                &mut receivers,
            )?);
        }
    }
    save(&mut deps.storage, CONFIG_KEY, &config)?;
    update_owner_inventory(&mut deps.storage, &inv_updates, num_perm_types)?;
    Ok(messages)
}


/// Returns StdResult<()>
///
/// returns Ok if the address has permission or an error if not
///
/// # Arguments
///
/// * `deps` - a reference to Extern containing all the contract's external dependencies
/// * `block` - a reference to the current BlockInfo
/// * `token` - a reference to the token
/// * `token_id` - token ID String slice
/// * `opt_sender` - a optional reference to the address trying to get access to the token
/// * `perm_type` - PermissionType we are checking
/// * `oper_for` - a mutable reference to a list of owners that gave the sender "all" permission
/// * `custom_err` - string slice of the error msg to return if not permitted
/// * `owner_is_public` - true if token ownership is public for this contract
#[allow(clippy::too_many_arguments)]
pub fn check_permission<S: Storage, A: Api, Q: Querier>(
    deps: &Extern<S, A, Q>,
    block: &BlockInfo,
    token: &Token,
    token_id: &str,
    opt_sender: Option<&CanonicalAddr>,
    perm_type: PermissionType,
    oper_for: &mut Vec<CanonicalAddr>,
    custom_err: &str,
    owner_is_public: bool,
) -> StdResult<()> {
    let exp_idx = perm_type.to_usize();
    let owner_slice = token.owner.as_slice();
    // check if owner is public/private.  use owner's setting if present, contract default
    // if not
    if let PermissionType::ViewOwner = perm_type {
        let priv_store = ReadonlyPrefixedStorage::new(PREFIX_OWNER_PRIV, &deps.storage);
        let pass: bool = may_load(&priv_store, owner_slice)?.unwrap_or(owner_is_public);
        if pass {
            return Ok(());
        }
    }
    check_perm_core(
        deps,
        block,
        token,
        token_id,
        opt_sender,
        owner_slice,
        exp_idx,
        oper_for,
        custom_err,
    )
}

/// Returns StdResult<()>
///
/// returns Ok if the address has permission or an error if not
///
/// # Arguments
///
/// * `deps` - a reference to Extern containing all the contract's external dependencies
/// * `block` - a reference to the current BlockInfo
/// * `token` - a reference to the token
/// * `token_id` - token ID String slice
/// * `opt_sender` - a optional reference to the address trying to get access to the token
/// * `owner_slice` - the owner of the token represented as a byte slice
/// * `exp_idx` - permission type we are checking represented as usize
/// * `oper_for` - a mutable reference to a list of owners that gave the sender "all" permission
/// * `custom_err` - string slice of the error msg to return if not permitted
#[allow(clippy::too_many_arguments)]
fn check_perm_core<S: Storage, A: Api, Q: Querier>(
    deps: &Extern<S, A, Q>,
    block: &BlockInfo,
    token: &Token,
    token_id: &str,
    opt_sender: Option<&CanonicalAddr>,
    owner_slice: &[u8],
    exp_idx: usize,
    oper_for: &mut Vec<CanonicalAddr>,
    custom_err: &str,
) -> StdResult<()> {
    // if did not already pass with "all" permission for this owner
    if !oper_for.contains(&token.owner) {
        let mut err_msg = custom_err;
        let mut expired_msg = String::new();
        let global_raw = CanonicalAddr(Binary::from(b"public"));
        let (sender, only_public) = if let Some(sdr) = opt_sender {
            (sdr, false)
        } else {
            (&global_raw, true)
        };
        // if this is the owner, all is good
        if token.owner == *sender {
            return Ok(());
        }
        // check if the token is public or the sender has token permission.
        // Can't use find because even if the global or sender permission expired, you
        // still want to see if the other is still valid, but if we are only checking for public
        // we can quit after one failure
        let mut one_expired = only_public;
        // if we are only checking for public permission, we can quit after one failure
        let mut found_one = only_public;
        for perm in &token.permissions {
            if perm.address == *sender || perm.address == global_raw {
                if let Some(exp) = perm.expirations[exp_idx] {
                    if !exp.is_expired(block) {
                        return Ok(());
                    // if the permission is expired
                    } else {
                        // if this is the sender let them know the permission expired
                        if perm.address != global_raw {
                            expired_msg
                                .push_str(&format!("Access to token {} has expired", token_id));
                            err_msg = &expired_msg;
                        }
                        // if both were expired (or only checking for global), there can't be any ALL permissions
                        // so just exit early
                        if one_expired {
                            return Err(StdError::generic_err(err_msg));
                        } else {
                            one_expired = true;
                        }
                    }
                }
                // we can quit if we found both the sender and the global (or only checking global)
                if found_one {
                    break;
                } else {
                    found_one = true;
                }
            }
        }
        // check if the entire permission type is public or the sender has ALL permission.
        // Can't use find because even if the global or sender permission expired, you
        // still want to see if the other is still valid, but if we are only checking for public
        // we can quit after one failure
        let all_store = ReadonlyPrefixedStorage::new(PREFIX_ALL_PERMISSIONS, &deps.storage);
        let may_list: Option<Vec<Permission>> = json_may_load(&all_store, owner_slice)?;
        found_one = only_public;
        if let Some(list) = may_list {
            for perm in &list {
                if perm.address == *sender || perm.address == global_raw {
                    if let Some(exp) = perm.expirations[exp_idx] {
                        if !exp.is_expired(block) {
                            oper_for.push(token.owner.clone());
                            return Ok(());
                        // if the permission expired and this is the sender let them know the
                        // permission expired
                        } else if perm.address != global_raw {
                            expired_msg.push_str(&format!(
                                "Access to all tokens of {} has expired",
                                &deps.api.human_address(&token.owner)?
                            ));
                            err_msg = &expired_msg;
                        }
                    }
                    // we can quit if we found both the sender and the global (or only checking global)
                    if found_one {
                        return Err(StdError::generic_err(err_msg));
                    } else {
                        found_one = true;
                    }
                }
            }
        }
        return Err(StdError::generic_err(err_msg));
    }
    Ok(())
}

/// Returns StdResult<(Token, u32)>
///
/// returns the token information if the sender has authorization
///
/// # Arguments
///
/// * `deps` - a reference to Extern containing all the contract's external dependencies
/// * `block` - a reference to the current BlockInfo
/// * `token_id` - token ID String slice
/// * `sender` - a optional reference to the address trying to get access to the token
/// * `perm_type` - PermissionType we are checking
/// * `oper_for` - a mutable reference to a list of owners that gave the sender "all" permission
/// * `config` - a reference to the Config
fn get_token_if_permitted<S: Storage, A: Api, Q: Querier>(
    deps: &Extern<S, A, Q>,
    block: &BlockInfo,
    token_id: &str,
    sender: Option<&CanonicalAddr>,
    perm_type: PermissionType,
    oper_for: &mut Vec<CanonicalAddr>,
    config: &Config,
) -> StdResult<(Token, u32)> {
    let custom_err = format!(
        "You are not authorized to perform this action on token {}",
        token_id
    );
    // if token supply is private, don't leak that the token id does not exist
    // instead just say they are not authorized for that token
    let opt_err = if config.token_supply_is_public {
        None
    } else {
        Some(&*custom_err)
    };
    let (token, idx) = get_token(&deps.storage, token_id, opt_err)?;
    check_permission(
        deps,
        block,
        &token,
        token_id,
        sender,
        perm_type,
        oper_for,
        &custom_err,
        config.owner_is_public,
    )?;
    Ok((token, idx))
}

/// Returns StdResult<(Token, u32)>
///
/// returns the specified token and its identifier index
///
/// # Arguments
///
/// * `storage` - a reference to contract's storage
/// * `token_id` - token id string slice
/// * `custom_err` - optional custom error message to use if don't want to reveal that a token
///                  does not exist
fn get_token<S: ReadonlyStorage>(
    storage: &S,
    token_id: &str,
    custom_err: Option<&str>,
) -> StdResult<(Token, u32)> {
    let default_err: String;
    let not_found = if let Some(err) = custom_err {
        err
    } else {
        default_err = format!("Token ID: {} not found", token_id);
        &*default_err
    };
    let map2idx = ReadonlyPrefixedStorage::new(PREFIX_MAP_TO_INDEX, storage);
    let idx: u32 =
        may_load(&map2idx, token_id.as_bytes())?.ok_or_else(|| StdError::generic_err(not_found))?;
    let info_store = ReadonlyPrefixedStorage::new(PREFIX_INFOS, storage);
    let token: Token = json_may_load(&info_store, &idx.to_le_bytes())?.ok_or_else(|| {
        StdError::generic_err(format!("Unable to find token info for {}", token_id))
    })?;
    Ok((token, idx))
}

/// Returns StdResult<bool> result of validating an address' viewing key
///
/// # Arguments
///
/// * `storage` - a reference to the contract's storage
/// * `address` - a reference to the address whose key should be validated
/// * `viewing_key` - String key used for authentication
fn check_key<S: ReadonlyStorage>(
    storage: &S,
    address: &CanonicalAddr,
    viewing_key: String,
) -> StdResult<()> {
    // load the address' key
    let read_key = ReadonlyPrefixedStorage::new(PREFIX_VIEW_KEY, storage);
    let load_key: [u8; VIEWING_KEY_SIZE] =
        may_load(&read_key, address.as_slice())?.unwrap_or([0u8; VIEWING_KEY_SIZE]);
    let input_key = ViewingKey(viewing_key);
    // if key matches
    if input_key.check_viewing_key(&load_key) {
        return Ok(());
    }
    Err(StdError::generic_err(
        "Wrong viewing key for this address or viewing key not set",
    ))
}

/// Returns StdResult<()>
///
/// returns Ok if authorized to view token supply, Err otherwise
///
/// # Arguments
///
/// * `deps` - a reference to Extern containing all the contract's external dependencies
/// * `viewer` - optional address and key making an authenticated query request
/// * `from_permit` - address derived from an Owner permit, if applicable
fn check_view_supply<S: Storage, A: Api, Q: Querier>(
    deps: &Extern<S, A, Q>,
    viewer: Option<ViewerInfo>,
    from_permit: Option<CanonicalAddr>,
) -> StdResult<()> {
    let config: Config = load(&deps.storage, CONFIG_KEY)?;
    let mut is_auth = config.token_supply_is_public;
    if !is_auth {
        let querier = get_querier(deps, viewer, from_permit)?;
        if let Some(viewer_raw) = querier {
            let minters: Vec<CanonicalAddr> =
                may_load(&deps.storage, MINTERS_KEY)?.unwrap_or_default();
            is_auth = minters.contains(&viewer_raw);
        }
        if !is_auth {
            return Err(StdError::generic_err(
                "The token supply of this contract is private",
            ));
        }
    }
    Ok(())
}

/// Returns StdResult<(Option<HumanAddr>, Vec<Cw721Approval>, u32)> which is the owner, list of transfer
/// approvals, and token index of the request token
///
/// # Arguments
///
/// * `deps` - a reference to Extern containing all the contract's external dependencies
/// * `token_id` - string slice of the token id
/// * `viewer` - optional address and key making an authenticated query request
/// * `include_expired` - optionally true if the Approval lists should include expired Approvals
/// * `from_permit` - address derived from an Owner permit, if applicable
fn process_cw721_owner_of<S: Storage, A: Api, Q: Querier>(
    deps: &Extern<S, A, Q>,
    token_id: &str,
    viewer: Option<ViewerInfo>,
    include_expired: Option<bool>,
    from_permit: Option<CanonicalAddr>,
) -> StdResult<(Option<HumanAddr>, Vec<Cw721Approval>, u32)> {
    let prep_info = query_token_prep(deps, token_id, viewer, from_permit)?;
    let opt_viewer = prep_info.viewer_raw.as_ref();
    if check_permission(
        deps,
        &prep_info.block,
        &prep_info.token,
        token_id,
        opt_viewer,
        PermissionType::ViewOwner,
        &mut Vec::new(),
        &prep_info.err_msg,
        prep_info.owner_is_public,
    )
    .is_ok()
    {
        let (owner, mut approvals, mut operators) = get_owner_of_resp(
            deps,
            &prep_info.block,
            &prep_info.token,
            opt_viewer,
            include_expired.unwrap_or(false),
        )?;
        approvals.append(&mut operators);
        return Ok((Some(owner), approvals, prep_info.idx));
    }
    Ok((None, Vec::new(), prep_info.idx))
}

/// Returns StdResult<TokenQueryInfo> after performing common preparations for authenticated
/// token queries
///
/// # Arguments
///
/// * `deps` - a reference to Extern containing all the contract's external dependencies
/// * `token_id` - string slice of the token id
/// * `viewer` - optional address and key making an authenticated query request
/// * `from_permit` - address derived from an Owner permit, if applicable
fn query_token_prep<S: Storage, A: Api, Q: Querier>(
    deps: &Extern<S, A, Q>,
    token_id: &str,
    viewer: Option<ViewerInfo>,
    from_permit: Option<CanonicalAddr>,
) -> StdResult<TokenQueryInfo> {
    let viewer_raw = get_querier(deps, viewer, from_permit)?;
    let config: Config = load(&deps.storage, CONFIG_KEY)?;
    // TODO remove this when BlockInfo becomes available to queries
    let block: BlockInfo = may_load(&deps.storage, BLOCK_KEY)?.unwrap_or_else(|| BlockInfo {
        height: 1,
        time: 1,
        chain_id: "not used".to_string(),
    });
    let err_msg = format!(
        "You are not authorized to perform this action on token {}",
        token_id
    );
    // if token supply is private, don't leak that the token id does not exist
    // instead just say they are not authorized for that token
    let opt_err = if config.token_supply_is_public {
        None
    } else {
        Some(&*err_msg)
    };
    let (token, idx) = get_token(&deps.storage, token_id, opt_err)?;
    Ok(TokenQueryInfo {
        viewer_raw,
        block,
        err_msg,
        token,
        idx,
        owner_is_public: config.owner_is_public,
    })
}

/// Returns StdResult<Option<CanonicalAddr>> from determining the querying address (if possible) either
/// from a permit validation or a ViewerInfo
///
/// # Arguments
///
/// * `deps` - a reference to Extern containing all the contract's external dependencies
/// * `viewer` - optional address and key making an authenticated query request
/// * `from_permit` - the address derived from an Owner permit, if applicable
fn get_querier<S: Storage, A: Api, Q: Querier>(
    deps: &Extern<S, A, Q>,
    viewer: Option<ViewerInfo>,
    from_permit: Option<CanonicalAddr>,
) -> StdResult<Option<CanonicalAddr>> {
    if from_permit.is_some() {
        return Ok(from_permit);
    }
    let viewer_raw = viewer
        .map(|v| {
            let raw = deps.api.canonical_address(&v.address)?;
            check_key(&deps.storage, &raw, v.viewing_key)?;
            Ok(raw)
        })
        .transpose()?;
    Ok(viewer_raw)
}

/// Returns StdResult<(HumanAddr, Vec<Cw721Approval>, Vec<Cw721Approval>)>
/// which is the owner, token transfer Approval list, and Approval list of everyone
/// that can transfer all of the token owner's tokens
///
/// # Arguments
///
/// * `deps` - a reference to Extern containing all the contract's external dependencies
/// * `block` - a reference to the current BlockInfo
/// * `token` - a reference to the token whose owner info is being requested
/// * `viewer` - optional reference to the address requesting to view the owner
/// * `include_expired` - true if the Approval lists should include expired Approvals
fn get_owner_of_resp<S: Storage, A: Api, Q: Querier>(
    deps: &Extern<S, A, Q>,
    block: &BlockInfo,
    token: &Token,
    viewer: Option<&CanonicalAddr>,
    include_expired: bool,
) -> StdResult<(HumanAddr, Vec<Cw721Approval>, Vec<Cw721Approval>)> {
    let owner = deps.api.human_address(&token.owner)?;
    let mut spenders: Vec<Cw721Approval> = Vec::new();
    let mut operators: Vec<Cw721Approval> = Vec::new();
    if let Some(vwr) = viewer {
        if token.owner == *vwr {
            let transfer_idx = PermissionType::Transfer.to_usize();
            gen_cw721_approvals(
                &deps.api,
                block,
                &token.permissions,
                &mut spenders,
                transfer_idx,
                include_expired,
            )?;
            let all_store = ReadonlyPrefixedStorage::new(PREFIX_ALL_PERMISSIONS, &deps.storage);
            let all_perm: Vec<Permission> =
                json_may_load(&all_store, token.owner.as_slice())?.unwrap_or_default();
            gen_cw721_approvals(
                &deps.api,
                block,
                &all_perm,
                &mut operators,
                transfer_idx,
                include_expired,
            )?;
        }
    }
    Ok((owner, spenders, operators))
}

// bundled info when prepping an authenticated token query
pub struct TokenQueryInfo {
    // querier's address
    viewer_raw: Option<CanonicalAddr>,
    // TODO remove this when BlockInfo becomes available to queries
    block: BlockInfo,
    // error message String
    err_msg: String,
    // the requested token
    token: Token,
    // the requested token's index
    idx: u32,
    // true if the contract has public ownership
    owner_is_public: bool,
}

/// Returns StdResult<()> resulting from generating the list of Approvals
///
/// # Arguments
///
/// * `api` - reference to the Api used to convert canonical and human addresses
/// * `block` - a reference to the current BlockInfo
/// * `perm_list` - slice of Permissions to search through looking for transfer approvals
/// * `approvals` - a mutable reference to the list of approvals that should be appended
///                 with any found in the permission list
/// * `transfer_idx` - index into the Permission expirations that represents transfers
/// * `include_expired` - true if the Approval list should include expired Approvals
fn gen_cw721_approvals<A: Api>(
    api: &A,
    block: &BlockInfo,
    perm_list: &[Permission],
    approvals: &mut Vec<Cw721Approval>,
    transfer_idx: usize,
    include_expired: bool,
) -> StdResult<()> {
    let global_raw = CanonicalAddr(Binary::from(b"public"));
    for perm in perm_list {
        if let Some(exp) = perm.expirations[transfer_idx] {
            if (include_expired || !exp.is_expired(block)) && perm.address != global_raw {
                approvals.push(Cw721Approval {
                    spender: api.human_address(&perm.address)?,
                    expires: exp,
                });
            }
        }
    }
    Ok(())
}
