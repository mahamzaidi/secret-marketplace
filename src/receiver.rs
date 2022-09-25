#![cfg_attr(debug_assertions, allow(dead_code, unused_imports))]
#![allow(warnings, unused)]
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use cosmwasm_storage::{PrefixedStorage};
use cosmwasm_std::{Binary, CosmosMsg, HumanAddr, StdResult, Storage, StdError, Api, Querier, Extern, };
use secret_toolkit::utils::HandleCallback;

use crate::contract::BLOCK_SIZE;
use crate::state::{save, Config, CONFIG_KEY, PREFIX_INFOS, PREFIX_TX_IDS, load};

/// used to create ReceiveNft and BatchReceiveNft callback messages.  BatchReceiveNft is preferred
/// over ReceiveNft, because ReceiveNft does not allow the recipient to know who sent the token,
/// only its previous owner, and ReceiveNft can only process one token.  So it is inefficient when
/// sending multiple tokens to the same contract (a deck of game cards for instance).  ReceiveNft
/// primarily exists just to maintain CW-721 compliance.  Also, it should be noted that the CW-721
/// `sender` field is inaccurately named, because it is used to hold the address the token came from,
/// not the address that sent it (which is not always the same).  The name is reluctantly kept in
/// ReceiveNft to maintain CW-721 compliance, but BatchReceiveNft uses `sender` to hold the sending
/// address (which matches both its true role and its SNIP-20 Receive counterpart).  Any contract
/// that is implementing both Receiver Interfaces must be sure that the ReceiveNft `sender` field
/// is actually processed like a BatchReceiveNft `from` field.  Again, apologies for any confusion
/// caused by propagating inaccuracies, but because InterNFT is planning on using CW-721 standards,
/// compliance with CW-721 might be necessary
#[derive(Serialize, Deserialize, Clone, PartialEq, JsonSchema, Debug)]
#[serde(rename_all = "snake_case")]
pub enum Snip721ReceiveMsg {
    /// ReceiveNft may be a HandleMsg variant of any contract that wants to implement a receiver
    /// interface.  BatchReceiveNft, which is more informative and more efficient, is preferred over
    /// ReceiveNft.  Please read above regarding why ReceiveNft, which follows CW-721 standard has an
    /// inaccurately named `sender` field
    ReceiveNft {
        /// previous owner of sent token
        sender: HumanAddr,
        /// token that was sent
        token_id: String,
        /// optional message to control receiving logic
        msg: Option<Binary>,
    },
    /// BatchReceiveNft may be a HandleMsg variant of any contract that wants to implement a receiver
    /// interface.  BatchReceiveNft, which is more informative and more efficient, is preferred over
    /// ReceiveNft.
    BatchReceiveNft {
        /// address that sent the tokens.  There is no ReceiveNft field equivalent to this
        sender: HumanAddr,
        /// previous owner of sent tokens.  This is equivalent to the ReceiveNft `sender` field
        from: HumanAddr,
        /// tokens that were sent
        token_ids: Vec<String>,
        /// optional message to control receiving logic
        msg: Option<Binary>,
    },
}

impl HandleCallback for Snip721ReceiveMsg {
    const BLOCK_SIZE: usize = BLOCK_SIZE;
}

/// Returns a StdResult<CosmosMsg> used to call a registered contract's ReceiveNft
///
/// # Arguments
///
/// * `sender` - the address of the former owner of the sent token
/// * `token_id` - ID String of the token that was sent
/// * `msg` - optional msg used to control ReceiveNft logic
/// * `callback_code_hash` - String holding the code hash of the contract that was
///                          sent the token
/// * `contract_addr` - address of the contract that was sent the token
pub fn receive_nft_msg<S: Storage, A: Api, Q: Querier>(
    deps: &mut Extern<S, A, Q>,
    sender: HumanAddr,
    token_id: String,
    msg: Option<Binary>,
    callback_code_hash: String,
    contract_addr: HumanAddr,
) -> StdResult<CosmosMsg> {
    let _sender = &sender;
    let _tokenId = &token_id;
   

    let mut config: Config = load(&deps.storage, CONFIG_KEY)?;
    config.token_cnt = config.token_cnt.checked_add(1).ok_or_else(|| {
        StdError::generic_err("Attempting to receive more tokens than the implementation limit")
    })?;
    ///save latest token count into the configuration
    save(&mut deps.storage, CONFIG_KEY, &config)?;
    let mut token_store = PrefixedStorage::new(PREFIX_INFOS, &mut deps.storage);
    /// save the receiver information which has implemented receive nft
    save(&mut token_store, _sender.to_string().as_bytes(), &_tokenId)?;
    // let mut add_ids = PrefixedStorage::new(PREFIX_TX_IDS, &mut deps.storage);
    // save(&mut add_ids, &token_id)?;

    let msg = Snip721ReceiveMsg::ReceiveNft {
        sender,
        token_id,
        msg,
    };
    msg.to_cosmos_msg(callback_code_hash, contract_addr, None)
}

/// Returns a StdResult<CosmosMsg> used to call a registered contract's
/// BatchReceiveNft
///
/// # Arguments
///
/// * `sender` - the address that is sending the token
/// * `from` - the address of the former owner of the sent token
/// * `token_ids` - list of ID Strings of the tokens that were sent
/// * `msg` - optional msg used to control ReceiveNft logic
/// * `callback_code_hash` - String holding the code hash of the contract that was
///                          sent the token
/// * `contract_addr` - address of the contract that was sent the token
pub fn batch_receive_nft_msg<S: Storage, A: Api, Q: Querier>(
    deps: &mut Extern<S, A, Q>,
    sender: HumanAddr,
    from: HumanAddr,
    token_ids: Vec<String>,
    msg: Option<Binary>,
    callback_code_hash: String,
    contract_addr: HumanAddr,
) -> StdResult<CosmosMsg> {
    let _sender = &sender;
    let _tokenIds = &token_ids;

    let mut config: Config = load(&deps.storage, CONFIG_KEY)?;
    // let received_tokens = token_ids.len();
    // config.token_cnt = config.token_cnt.checked_add(received_tokens).ok_or_else(|| {
    //     StdError::generic_err("Attempting to receive more tokens than the implementation limit")
    // })?;

    ///save latest token count into the configuration
    save(&mut deps.storage, CONFIG_KEY, &config)?;
    let mut token_store = PrefixedStorage::new(PREFIX_INFOS, &mut deps.storage);
    /// save the receiver information which has implemented receive nft
    save(&mut token_store, _sender.to_string().as_bytes(), &_tokenIds)?;
    // let mut add_ids = PrefixedStorage::new(PREFIX_TX_IDS, &mut deps.storage);
    // for id in &token_ids {
    //     save(&mut add_ids, &id)?;
    // }
    let msg = Snip721ReceiveMsg::BatchReceiveNft {
        sender,
        from,
        token_ids,
        msg,
    };
    msg.to_cosmos_msg(callback_code_hash, contract_addr, None)
}
