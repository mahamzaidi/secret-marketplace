# marketplace

General steps involving snip721 reference impl and the marketplace contract

1) Deploy the snip721 contract and call its create_viewing_key function.

2) Save the code id, contract address, the deployer's address and the viewing key. Every user of the contract must create their own viewing key so that they can carry out special queries that require authentication. Also, private metadata is only visible by using the owner's viewing keys.

3) Deloy the market place contract using another wallet address and call its create_viewing_key function. For simplicity you can keep the same wallet for both the deployments.

4) Save the code id, contract address, deployer's address and the viewing key.

5) In order to receive snip-721 tokens, the marketplace contract will register itself with the snip721 contract first by calling snip-721's register_receive_nft function.

6) Once registered, a user can use either the transfer_nft function or the send_nft function to transfer his 721 nft from his address to the marketplace contract or any other address. The only difference between transfer_nft and send_nft is that with send_nft you can also send an optional msg.

7) The transfer_nft/send_nft will in return call the receiver interface of the marketplace contract. If BatchReceiveNft function is implemented, that will be used by default as it gives us both sender and owner information of the token. If not then ReceiveNft is used which only gives the owner information. Check out more information about the differece in secret network documentation for receiver interface.

8) The marketplace contract has been coded as such that it will call list_nft function from the receiver functions. Thus all the nfts sent to this contract will be directly listed on the contract. 

## For the required HandleMsg and HandleAnswer:
Go to **msg.rs** file. The required HandleMsgs for first 6 steps are :

-RegisterReceiveNft, 
-TransferNft,
-SendNft,
-CreateViewingKey

The required HandleAnswers for first 6 steps are:

-RegisterReceiveNft,
-TransferNft,
-SendNft,
-CreateViewingKey

The HandleAnswers return an Enum variable which is defined in the msg.rs as this

```
pub enum ResponseStatus {
    Success,
    Failure,
}

```

###Note
1. If you create wallet addresses on the old version of secretcli, the addresses wont work for deployment and instantiation of contracts on the latest version of pulsar-2. You need to create new wallet addresses on the latest version of secretcli.
2. Scrt faucet https://faucet.pulsar.scrttestnet.com/
3. Pulsar-2 block explorer https://testnet.ping.pub/secret
4. Available endpoinst https://github.com/scrtlabs/api-registry#api-endpoints-1
