# marketplace

This contract extends the Snip-721 reference implementation.

The new functions implemented are:
- **SetSaleStatus**: This function is used to set the sale status of a token to either ForSale or NotForSale. The price can also be optionally set in this function. If no price is provided the price is set to 0. Only owner of token can perform this function. Returns the token_id and sale_status in the data field of HandleResponse. Token id whose sale status has been changed can be viewed in the logs returned by HandleResponse as well. 
- **SetPrice**: This function is used to set or update the price of the token. The function throws an error if 0 is provided as a price. The price is set only when the sale status is set to ForSale. Only owner of the token can perform this function. Returns token_id and token_price in data field of HandleResponse. Token id whose price has been changed can be viewed in the logs returned by HandleResponse as well.
- **BuyToken**: This function is used to buy a token. Returns token_id, buyer and the price for which the token was bought in the data field of HandleResponse. Anyone can buy the token if it is up for sale and if correct amount has been provided. After the execution of this function, the buyer will become the new owner of the token. A token which is not up for sale, or non-transferable and whose price is 0 cannot be bought.
- **TokensForSale**: Returns a list of token_ids which are currently up for sale.
- **SaleInfo**: Returns a token's sale status and its price.
- **BatchReceiveNft**: Is called by a snip721 contract whenever it sends nfts to our contract.
- **RegisterContractWithSnip721**: Registers our contract with another snip721 contract.

## Procedure:

1) After instantiating the contract, mint a token by calling the mint_nft function. E.g.
```
secretcli tx compute execute $CONTRACT '{"mint_nft": {"token_id":"1","transferable":true}}' --from keplr --keyring-backend test
```
2) Set the sale status of a token by calling set_sale_status function. You can also set the price in this function. However, if the status that is being set is not_for_sale then the price is always set to 0 whether you provide the price or not. E.g.
```
secretcli tx compute execute $CONTRACT '{"set_sale_status": {"token_id":"1","sale_status":"for_sale","price":1}}' --from keplr --keyring-backend test
```
3) To set or update the price call set_price function. E.g.
```
secretcli tx compute execute $CONTRACT '{"set_price": {"token_id":"1","price":3}}' --from keplr --keyring-backend test
```
4? To buy a token use this command:
```
secretcli tx compute execute $CONTRACT '{"buy_token": {"token_id":"1"}}' --amount 1000000uscrt --from bob --keyring-backend test
```
5) Check the current sale status of a token by querying the contract with sale_info. E.g.
```
secretcli query compute query $CONTRACT '{"sale_info": {"token_id":"1"}}'
```
6) Query current list of tokens up for sale by calling tokens_for_sale. E.g.
```
secretcli query compute query $CONTRACT '{"tokens_for_sale":{}}'
```

## Example transactions

These are the transaction hashes generated from the above procedure for contract **secret18sxfzndk6hy4czu54ncv0aynfc22s46d5tyx9h** and code_id **15218** for the execution messages. The contract admin is **secret1f2xhf3ruydr7latjyypx6x08enattstqdertks**

1) 22BFEEB76552565775BA03BDCC99AEB1F657D5C3986E2DA537A965A97D6093C8
2) 9DEA967A881148636BAFDBAFE837603A8120B2F023CA2AF7AE27E2F2445C60CE
3) 8D812545824A57DBB2560C133094DFDDCE6365D1D6EC53E5D8CAD5B7FF3D4DF6
4) B1B016A195E48D5B96E5025B8D3B716C1CAE1EE75232D9D2C297D011AFBF581C


### Note
1. If you create wallet addresses on the old version of secretcli, the addresses wont work for deployment and instantiation of contracts on the latest version of pulsar-2. You need to create new wallet addresses on the latest version of secretcli.
2. Scrt faucet https://faucet.pulsar.scrttestnet.com/
3. Pulsar-2 block explorer https://testnet.ping.pub/secret
4. Available endpoints https://github.com/scrtlabs/api-registry#api-endpoints-1
5. The contract is currently designed to only work with uscrt denom.
