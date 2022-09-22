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
use secret_toolkit::{
    permit::{validate, Permit, RevokedPermits},
    utils::{pad_handle_result, pad_query_result},
};
/// This contract implements SNIP-721 standard:
/// https://github.com/SecretFoundation/SNIPs/blob/master/SNIP-721.md
use std::collections::HashSet;

use crate::inventory::{Inventory, InventoryIter};
use crate::msg::{
    AccessLevel, ContractStatus, HandleAnswer, HandleMsg, InitMsg, QueryAnswer, QueryMsg,
    ReceiverInfo,
    ResponseStatus::{Failure, Success},
    Send, Transfer,
};
use crate::state::{
    json_may_load, json_save, load, may_load, remove, save, store_transfer, AuthList, Config,Permission, PermissionType, ReceiveRegistration, BLOCK_KEY, CONFIG_KEY, CREATOR_KEY,
    MY_ADDRESS_KEY, PREFIX_ALL_PERMISSIONS, PREFIX_AUTHLIST, PREFIX_INFOS, PREFIX_MAP_TO_ID,
    PREFIX_MAP_TO_INDEX, PREFIX_OWNER_PRIV, PREFIX_RECEIVERS, PREFIX_SELLERS, PREFIX_TX_IDS,
    PRNG_SEED_KEY,
};
use crate::token::Token;

pub const MARKET_FEE: u128 = 1000; //WRITE IN LOWEST DENOMINATION OF YOUR PREFERRED SNIP
pub const TOKEN_FEE: u128 = 1000000;
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
    let init_config = msg.config.unwrap_or_default();

    let config = Config {
        name: msg.name,
        admin: admin_raw.clone(),
        token_cnt: 0,
        tx_cnt: 0,
        status: ContractStatus::Normal.to_u8(),
        token_supply_is_public: init_config.public_token_supply.unwrap_or(true),
        owner_is_public: init_config.public_owner.unwrap_or(true),
    };

    let count = 0;

    save(&mut deps.storage, CONFIG_KEY, &config)?;
    save(&mut deps.storage, PRNG_SEED_KEY, &prng_seed)?;

    // TODO remove this after BlockInfo becomes available to queries
    save(&mut deps.storage, BLOCK_KEY, &env.block)?;

    // perform the post init callback if needed
    let messages: Vec<CosmosMsg> = if let Some(callback) = msg.post_init_callback {
        let execute = WasmMsg::Execute {
            msg: callback.msg,
            contract_addr: callback.contract_address,
            callback_code_hash: callback.code_hash,
            send: callback.send,
        };
        vec![execute.into()]
    } else {
        Vec::new()
    };
    Ok(InitResponse {
        messages,
        log: vec![],
    })
}
// list of tokens sent from one previous owner
pub struct SendFrom {
    // the owner's address
    pub owner: HumanAddr,
    // the tokens that were sent
    pub token_ids: Vec<String>,
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
        }
        HandleMsg::BatchReceiveNft {
            sender,
            from,
            token_ids,
            msg,
            code_hash,
            ..
        } => try_receive(deps, sender, from, &token_ids, msg, code_hash),
    };
    pad_handle_result(response, BLOCK_SIZE)
}

pub fn try_receive<S: Storage, A: Api, Q: Querier>(
    deps: &mut Extern<S, A, Q>,
    sender: HumanAddr,
    from: HumanAddr,
    token_ids: &[String],
    msg: Option<Binary>,
    code_hash: String,
) -> HandleResult {
    if token_ids.len() != 1 {
        return Err(StdError::generic_err("You may only send one token!"));
    }
    let mut config: Config = load(&deps.storage, CONFIG_KEY)?;

    // let sender_raw = deps.api.canonical_address(&env.message.sender)?;
    // let contract_addr = deps.api.canonical_address(&env.contract);
    //let amount_sent = env.message.sent_funds;

    // increment token count
    config.token_cnt = config.token_cnt.checked_add(1).ok_or_else(|| {
        StdError::generic_err("Attempting to receive more tokens than the implementation limit")
    })?;
    save(&mut deps.storage, CONFIG_KEY, &config)?;

    let mut token_store = PrefixedStorage::new(PREFIX_SELLERS, &mut deps.storage);
    save(&mut token_store, SELLER.to_string().as_bytes(), &token_ids)?;

    let res = HandleResponse {
        messages: vec![],
        log: vec![],
        data: Some(to_binary(&HandleAnswer::BatchReceiveNft {
            status: Success,
        })?),
    };
    Ok(res)
}

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
    };
    pad_query_result(response, BLOCK_SIZE)
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
    if !token.transferrable {
        return Err(StdError::generic_err(format!(
            "Token ID: {} is non-transferrable",
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
) -> HandleResult {
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

    for send_from in send_from_list.into_iter() {
        // if BatchReceiveNft is implemented, use it
        if impl_batch {
            return Ok(HandleResponse {
                messages: vec![],
                log: vec![],
                data: Some(to_binary(&HandleAnswer::BatchReceiveNft {
                    status: Success,
                })?),
            });
        }
    }
    Ok(HandleResponse {
        messages: vec![],
        log: vec![],
        data: Some(to_binary(&HandleAnswer::BatchReceiveNft {
            status: Failure,
        })?),
    })
}

// /// Returns StdResult<Vec<CosmosMsg>>
// ///
// /// transfer or sends a list of tokens and returns a list of ReceiveNft callbacks if applicable
// ///
// /// # Arguments
// ///
// /// * `deps` - a mutable reference to Extern containing all the contract's external dependencies
// /// * `env` - a reference to the Env of the contract's environment
// /// * `config` - a mutable reference to the Config
// /// * `sender` - a reference to the message sender address
// /// * `transfers` - optional list of transfers to perform
// /// * `sends` - optional list of sends to perform
// fn send_list<S: Storage, A: Api, Q: Querier>(
//     deps: &mut Extern<S, A, Q>,
//     env: &Env,
//     config: &mut Config,
//     sender: &CanonicalAddr,
//     transfers: Option<Vec<Transfer>>,
//     sends: Option<Vec<Send>>,
// ) -> StdResult<Vec<CosmosMsg>> {
//     let mut messages: Vec<CosmosMsg> = Vec::new();
//     let mut oper_for: Vec<CanonicalAddr> = Vec::new();
//     let mut inv_updates: Vec<InventoryUpdate> = Vec::new();
//     let num_perm_types = PermissionType::ViewOwner.num_types();
//     if let Some(xfers) = transfers {
//         for xfer in xfers.into_iter() {
//             let recipient_raw = deps.api.canonical_address(&xfer.recipient)?;
//             for token_id in xfer.token_ids.into_iter() {
//                 let _o = transfer_impl(
//                     deps,
//                     &env.block,
//                     config,
//                     sender,
//                     token_id,
//                     recipient_raw.clone(),
//                     &mut oper_for,
//                     &mut inv_updates,
//                     xfer.memo.clone(),
//                 )?;
//             }
//         }
//     } else if let Some(snds) = sends {
//         let mut receivers = Vec::new();
//         for send in snds.into_iter() {
//             let contract_raw = deps.api.canonical_address(&send.contract)?;
//             let mut send_from_list: Vec<SendFrom> = Vec::new();
//             for token_id in send.token_ids.into_iter() {
//                 let owner_raw = transfer_impl(
//                     deps,
//                     &env.block,
//                     config,
//                     sender,
//                     token_id.clone(),
//                     contract_raw.clone(),
//                     &mut oper_for,
//                     &mut inv_updates,
//                     send.memo.clone(),
//                 )?;
//                 // compile list of all tokens being sent from each owner in this Send
//                 let owner = deps.api.human_address(&owner_raw)?;
//                 if let Some(sd_fm) = send_from_list.iter_mut().find(|s| s.owner == owner) {
//                     sd_fm.token_ids.push(token_id.clone());
//                 } else {
//                     let new_sd_fm = SendFrom {
//                         owner,
//                         token_ids: vec![token_id.clone()],
//                     };
//                     send_from_list.push(new_sd_fm);
//                 }
//             }
//             // get BatchReceiveNft and ReceiveNft msgs for all the tokens sent in this Send
//             messages.extend(receiver_callback_msgs(
//                 &mut deps,
//                 env,
//                 &send.contract,
//                 &contract_raw,
//                 send.receiver_info,
//                 send_from_list,
//                 &send.msg,
//                 &env.message.sender,
//                 &mut receivers,
//             )?);
//         }
//     }
//     save(&mut deps.storage, CONFIG_KEY, &config)?;
//     update_owner_inventory(&mut deps.storage, &inv_updates, num_perm_types)?;
//     Ok(messages)
// }
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
