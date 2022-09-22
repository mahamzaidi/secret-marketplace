#![allow(clippy::large_enum_variant)]
#![cfg_attr(debug_assertions, allow(dead_code, unused_imports))]
#![allow(warnings, unused)]
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use cosmwasm_std::{Binary, Coin, HumanAddr, Uint128};


/// Instantiation message
#[derive(Serialize, Deserialize, JsonSchema)]
pub struct InitMsg {
    /// name of token contract
    pub name: String,
    /// optional admin address, env.message.sender if missing
    pub admin: Option<HumanAddr>,
    /// optional privacy configuration for the contract
    pub config: Option<InitConfig>,
    /// entropy used for prng seed
    pub entropy: String,
    /// optional callback message to execute after instantiation.  This will
    /// most often be used to have the token contract provide its address to a
    /// contract that instantiated it, but it could be used to execute any
    /// contract
    pub post_init_callback: Option<PostInitCallback>,
}

/// This type represents optional configuration values.
/// All values are optional and have defaults which are more private by default,
/// but can be overridden if necessary
#[derive(Serialize, Deserialize, JsonSchema, Clone, Debug)]
pub struct InitConfig {
    /// indicates whether the token IDs and the number of tokens controlled by the contract are
    /// public.  If the token supply is private, only minters can view the token IDs and
    /// number of tokens controlled by the contract
    /// default: False
    pub public_token_supply: Option<bool>,
    /// indicates whether token ownership is public or private.  A user can still change whether the
    /// ownership of their tokens is public or private
    /// default: False
    pub public_owner: Option<bool>,
}

impl Default for InitConfig {
    fn default() -> Self {
        InitConfig {
            public_token_supply: Some(true),
            public_owner: Some(true),
        }
    }
}

/// info needed to perform a callback message after instantiation
#[derive(Serialize, Deserialize, JsonSchema, Clone, Debug)]
pub struct PostInitCallback {
    /// the callback message to execute
    pub msg: Binary,
    /// address of the contract to execute
    pub contract_address: HumanAddr,
    /// code hash of the contract to execute
    pub code_hash: String,
    /// list of native Coin to send with the callback message
    pub send: Vec<Coin>,
}

/// token transfer info used when doing a BatchTransferNft
#[derive(Serialize, Deserialize, JsonSchema, Clone, Debug)]
pub struct Transfer {
    /// recipient of the transferred tokens
    pub recipient: HumanAddr,
    /// tokens being transferred
    pub token_ids: Vec<String>,
    /// optional memo for the tx
    pub memo: Option<String>,
}

#[derive(Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum HandleMsg {
    /// register that the message sending contract implements m and possibly
    /// BatchReceiveNft.  If a contract implements BatchReceiveNft, SendNft will always
    /// call BatchReceiveNft even if there is only one token transferred (the token_ids
    /// Vec will only contain one ID)
    RegisterReceiveNft {
        /// receving contract's code hash
        code_hash: String,
        /// optionally true if the contract also implements BatchReceiveNft.  Defaults
        /// to false if not specified
        also_implements_batch_receive_nft: Option<bool>,
        /// optional message length padding
        padding: Option<String>,
    },
    /// set contract status level to determine which functions are allowed.  StopTransactions
    /// status prevent mints, burns, sends, and transfers, but allows all other functions
    SetContractStatus {
        /// status level
        level: ContractStatus,
        /// optional message length padding
        padding: Option<String>,
    },
    BatchReceiveNft {
        /// address that sent the NFTs
        sender: HumanAddr,
        /// previous owner of the NFTs
        from: HumanAddr,
        /// list of NFTs sent from the previous owner
        token_ids: Vec<String>,
        /// msg specified when sending
        msg: Option<Binary>,
        /// receving contract's code hash
        code_hash: String,
    },

}

/// send token info used when doing a BatchSendNft
#[derive(Serialize, Deserialize, JsonSchema, Clone, Debug)]
pub struct Send {
    /// recipient of the sent tokens
    pub contract: HumanAddr,
    /// optional code hash and BatchReceiveNft implementation status of the recipient contract
    pub receiver_info: Option<ReceiverInfo>,
    /// tokens being sent
    pub token_ids: Vec<String>,
    /// optional message to send with the (Batch)RecieveNft callback
    pub msg: Option<Binary>,
    /// optional memo for the tx
    pub memo: Option<String>,
}

/// permission access level
#[derive(Serialize, Deserialize, JsonSchema, Debug)]
#[serde(rename_all = "snake_case")]
pub enum AccessLevel {
    /// approve permission only for the specified token
    ApproveToken,
    /// grant permission for all tokens
    All,
    /// revoke permission only for the specified token
    RevokeToken,
    /// remove all permissions for this address
    None,
}

#[derive(Serialize, Deserialize, JsonSchema, Debug)]
#[serde(rename_all = "snake_case")]
pub enum HandleAnswer {
    RegisterReceiveNft {
        status: ResponseStatus,
    },
    SetContractStatus {
        status: ResponseStatus,
    },
    BatchReceiveNft {
        status: ResponseStatus,
    }
}

/// the address and viewing key making an authenticated query request
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
pub struct ViewerInfo {
    /// querying address
    pub address: HumanAddr,
    /// authentication key string
    pub viewing_key: String,
}

/// a recipient contract's code hash and whether it implements BatchReceiveNft
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
pub struct ReceiverInfo {
    /// recipient's code hash
    pub recipient_code_hash: String,
    /// true if the contract also implements BacthReceiveNft.  Defaults to false
    /// if not specified
    pub also_implements_batch_receive_nft: Option<bool>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum QueryMsg {
    /// display the contract's name and symbol
    ContractInfo {},
    /// display the contract's configuration
    ContractConfig {},
    /// display the code hash a contract has registered with the token contract and whether
    /// the contract implements BatchReceivenft
    RegisteredCodeHash {
        /// the contract whose receive registration info you want to view
        contract: HumanAddr,
    },
    /// display the contract's creator
    ContractCreator {},
}

#[derive(Serialize, Deserialize, JsonSchema, Debug)]
#[serde(rename_all = "snake_case")]
pub enum QueryAnswer {
    ContractInfo {
        name: String,
    },
    ContractConfig {
        token_supply_is_public: bool,
        owner_is_public: bool,
    },
    RegisteredCodeHash {
        code_hash: Option<String>,
        also_implements_batch_receive_nft: bool,
    },
    ContractCreator {
        creator: Option<HumanAddr>,
    },
}

#[derive(Serialize, Deserialize, Clone, PartialEq, JsonSchema, Debug)]
#[serde(rename_all = "snake_case")]
pub enum ResponseStatus {
    Success,
    Failure,
}

#[derive(Serialize, Deserialize, Clone, PartialEq, JsonSchema, Debug)]
#[serde(rename_all = "snake_case")]
pub enum ContractStatus {
    Normal,
    StopTransactions,
    StopAll,
}

/// tx type and specifics
#[derive(Serialize, Deserialize, JsonSchema, Clone, PartialEq, Debug)]
#[serde(rename_all = "snake_case")]
pub enum TxAction {
    /// transferred token ownership
    Transfer {
        /// previous owner
        from: HumanAddr,
        /// optional sender if not owner
        sender: Option<HumanAddr>,
        /// new owner
        recipient: HumanAddr,
    },

}

/// tx for display
#[derive(Serialize, Deserialize, JsonSchema, Clone, PartialEq, Debug)]
#[serde(rename_all = "snake_case")]
pub struct Tx {
    /// tx id
    pub tx_id: u64,
    /// the block containing this tx
    pub block_height: u64,
    /// the time (in seconds since 01/01/1970) of the block containing this tx
    pub block_time: u64,
    /// token id
    pub token_id: String,
    /// tx type and specifics
    pub action: TxAction,
    /// optional memo
    pub memo: Option<String>,
}

impl ContractStatus {
    /// Returns u8 representation of the ContractStatus
    pub fn to_u8(&self) -> u8 {
        match self {
            ContractStatus::Normal => 0,
            ContractStatus::StopTransactions => 1,
            ContractStatus::StopAll => 2,
        }
    }
}

