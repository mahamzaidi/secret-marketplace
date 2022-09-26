#![allow(clippy::large_enum_variant)]
#![cfg_attr(debug_assertions, allow(dead_code, unused_imports))]
#![allow(warnings, unused)]
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use cosmwasm_std::{Binary, Coin, HumanAddr, Uint128};
use crate::expiration::Expiration;
use crate::token::{Extension, Metadata};
use secret_toolkit::permit::Permit;



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
    MakeOwnershipPrivate {
        /// optional message length padding
        padding: Option<String>,
    },
    TransferNft {
        /// recipient of the transfer
        recipient: HumanAddr,
        /// id of the token to transfer
        token_id: String,
        /// optional memo for the tx
        memo: Option<String>,
        /// optional message length padding
        padding: Option<String>,
    },
    SendNft {
        /// address to send the token to
        contract: HumanAddr,
        /// optional code hash and BatchReceiveNft implementation status of the recipient contract
        receiver_info: Option<ReceiverInfo>,
        /// id of the token to send
        token_id: String,
        /// optional message to send with the (Batch)RecieveNft callback
        msg: Option<Binary>,
        /// optional memo for the tx
        memo: Option<String>,
        /// optional message length padding
        padding: Option<String>,
    },
    CreateViewingKey {
        /// entropy String used in random key generation
        entropy: String,
        /// optional message length padding
        padding: Option<String>,
    },
    SetViewingKey {
        /// desired viewing key
        key: String,
        /// optional message length padding
        padding: Option<String>,
    },
    ChangeAdmin {
        /// address with admin authority
        address: HumanAddr,
        /// optional message length padding
        padding: Option<String>,
    },
    ListNft{
        token_id: String,
        sale_price: u32,
        available_for_auction: bool, 
        msg: Option<Binary>,
        memo: Option<String>,      
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
pub enum AuctionStatus {
    NotUpForAuction,
    AuctionInProgress,
    AuctionClosed,
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
    MakeOwnershipPrivate {
        status: ResponseStatus,
    },
    TransferNft {
        status: ResponseStatus,
    },
    SendNft {
        status: ResponseStatus,
    },
    /// response from both setting and creating a viewing key
    ViewingKey {
        key: String,
    },
    ChangeAdmin {
        status: ResponseStatus,
    },
    ListNft {
        status: ResponseStatus,
    }
}

/// response of CW721 OwnerOf
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
pub struct Cw721OwnerOfResponse {
    /// Owner of the token if permitted to view it
    pub owner: Option<HumanAddr>,
    /// list of addresses approved to transfer this token
    pub approvals: Vec<Cw721Approval>,
}

/// CW721 Approval
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
pub struct Cw721Approval {
    /// address that can transfer the token
    pub spender: HumanAddr,
    /// expiration of this approval
    pub expires: Expiration,
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
    /// display the number of tokens controlled by the contract.  The token supply must
    /// either be public, or the querier must be an authenticated minter
    NumTokens {
        /// optional address and key requesting to view the number of tokens
        viewer: Option<ViewerInfo>,
    },
    /// display an optionally paginated list of all the tokens controlled by the contract.
    /// The token supply must either be public, or the querier must be an authenticated
    /// minter
    AllTokens {
        /// optional address and key requesting to view the list of tokens
        viewer: Option<ViewerInfo>,
        /// paginate by providing the last token_id received in the previous query
        start_after: Option<String>,
        /// optional number of token ids to display
        limit: Option<u32>,
    },
    /// display the owner of the specified token if authorized to view it.  If the requester
    /// is also the token's owner, the response will also include a list of any addresses
    /// that can transfer this token.  The transfer approval list is for CW721 compliance,
    /// but the NftDossier query will be more complete by showing viewing approvals as well
    OwnerOf {
        token_id: String,
        /// optional address and key requesting to view the token owner
        viewer: Option<ViewerInfo>,
        /// optionally include expired Approvals in the response list.  If ommitted or
        /// false, expired Approvals will be filtered out of the response
        include_expired: Option<bool>,
    },
    /// displays the public metadata of a token
    NftInfo { token_id: String },
    /// displays all the information contained in the OwnerOf and NftInfo queries
    AllNftInfo {
        token_id: String,
        /// optional address and key requesting to view the token owner
        viewer: Option<ViewerInfo>,
        /// optionally include expired Approvals in the response list.  If ommitted or
        /// false, expired Approvals will be filtered out of the response
        include_expired: Option<bool>,
    },
    /// displays the private metadata if permitted to view it
    PrivateMetadata {
        token_id: String,
        /// optional address and key requesting to view the private metadata
        viewer: Option<ViewerInfo>,
    },
    /// displays a list of all the tokens belonging to the input owner in which the viewer
    /// has view_owner permission
    Tokens {
        owner: HumanAddr,
        /// optional address of the querier if different from the owner
        viewer: Option<HumanAddr>,
        /// optional viewing key
        viewing_key: Option<String>,
        /// paginate by providing the last token_id received in the previous query
        start_after: Option<String>,
        /// optional number of token ids to display
        limit: Option<u32>,
    },
    /// displays the number of tokens that the querier has permission to see the owner and that
    /// belong to the specified address
    NumTokensOfOwner {
        owner: HumanAddr,
        /// optional address of the querier if different from the owner
        viewer: Option<HumanAddr>,
        /// optional viewing key
        viewing_key: Option<String>,
    },
     /// display if a token is transferable
     IsTransferable { token_id: String },
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
    OwnerOf {
        owner: HumanAddr,
        approvals: Vec<Cw721Approval>,
    },
    NftInfo {
        token_uri: Option<String>,
        extension: Option<Extension>,
    },
    PrivateMetadata {
        token_uri: Option<String>,
        extension: Option<Extension>,
    },
    AllNftInfo {
        access: Cw721OwnerOfResponse,
        info: Option<Metadata>,
    },
    IsTransferable {
        token_is_transferable: bool,
    },
    TokenList {
        tokens: Vec<String>,
    },
    NumTokens {
        count: u32,
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

/// queries using permits instead of viewing keys
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum QueryWithPermit {
    /// display the royalty information of a token if a token ID is specified, or display the
    /// contract's default royalty information in no token ID is provided
    // RoyaltyInfo {
    //     /// optional ID of the token whose royalty information should be displayed.  If not
    //     /// provided, display the contract's default royalty information
    //     token_id: Option<String>,
    // },
    /// displays the private metadata if permitted to view it
    PrivateMetadata { token_id: String },
    /// displays all the information about a token that the viewer has permission to
    /// see.  This may include the owner, the public metadata, the private metadata, royalty
    /// information, mint run information, whether the token is unwrapped, whether the token is
    /// transferable, and the token and inventory approvals
    // NftDossier {
    //     token_id: String,
    //     /// optionally include expired Approvals in the response list.  If ommitted or
    //     /// false, expired Approvals will be filtered out of the response
    //     include_expired: Option<bool>,
    // },
    /// displays all the information about multiple tokens that the viewer has permission to
    /// see.  This may include the owner, the public metadata, the private metadata, royalty
    /// information, mint run information, whether the token is unwrapped, whether the token is
    /// transferable, and the token and inventory approvals
    // BatchNftDossier {
    //     token_ids: Vec<String>,
    //     /// optionally include expired Approvals in the response list.  If ommitted or
    //     /// false, expired Approvals will be filtered out of the response
    //     include_expired: Option<bool>,
    // },
    /// display the owner of the specified token if authorized to view it.  If the requester
    /// is also the token's owner, the response will also include a list of any addresses
    /// that can transfer this token.  The transfer approval list is for CW721 compliance,
    /// but the NftDossier query will be more complete by showing viewing approvals as well
    OwnerOf {
        token_id: String,
        /// optionally include expired Approvals in the response list.  If ommitted or
        /// false, expired Approvals will be filtered out of the response
        include_expired: Option<bool>,
    },
    /// displays all the information contained in the OwnerOf and NftInfo queries
    AllNftInfo {
        token_id: String,
        /// optionally include expired Approvals in the response list.  If ommitted or
        /// false, expired Approvals will be filtered out of the response
        include_expired: Option<bool>,
    },
    /// list all the inventory-wide approvals in place for the permit creator
    // InventoryApprovals {
    //     /// optionally include expired Approvals in the response list.  If ommitted or
    //     /// false, expired Approvals will be filtered out of the response
    //     include_expired: Option<bool>,
    // },
    /// verify that the permit creator has approval to transfer every listed token.  
    /// A token will count as unapproved if it is non-transferable
    // VerifyTransferApproval {
    //     /// list of tokens to verify approval for
    //     token_ids: Vec<String>,
    // },
    /// display the transaction history for the permit creator in reverse
    /// chronological order
    // TransactionHistory {
    //     /// optional page to display
    //     page: Option<u32>,
    //     /// optional number of transactions per page
    //     page_size: Option<u32>,
    // },
    /// display the number of tokens controlled by the contract.  The token supply must
    /// either be public, or the querier must be an authenticated minter
    NumTokens {},
    /// display an optionally paginated list of all the tokens controlled by the contract.
    /// The token supply must either be public, or the querier must be an authenticated
    /// minter
    AllTokens {
        /// paginate by providing the last token_id received in the previous query
        start_after: Option<String>,
        /// optional number of token ids to display
        limit: Option<u32>,
    },
    /// list all the approvals in place for a specified token if given the owner's permit
    // TokenApprovals {
    //     token_id: String,
    //     /// optionally include expired Approvals in the response list.  If ommitted or
    //     /// false, expired Approvals will be filtered out of the response
    //     include_expired: Option<bool>,
    // },
    /// displays a list of all the CW721-style operators (any address that was granted
    /// approval to transfer all of the owner's tokens).  This query is provided to maintain
    /// CW721 compliance
    // ApprovedForAll {
    //     /// optionally include expired Approvals in the response list.  If ommitted or
    //     /// false, expired Approvals will be filtered out of the response
    //     include_expired: Option<bool>,
    // },
    /// displays a list of all the tokens belonging to the input owner in which the permit
    /// creator has view_owner permission
    Tokens {
        owner: HumanAddr,
        /// paginate by providing the last token_id received in the previous query
        start_after: Option<String>,
        /// optional number of token ids to display
        limit: Option<u32>,
    },
    /// displays the number of tokens that the querier has permission to see the owner and that
    /// belong to the specified address
    NumTokensOfOwner { owner: HumanAddr },
}