# Commands to get the smart contract up and running!

## Deployment

1) cargo build

2) systemctl start docker 

3) docker run --rm -v "$(pwd)":/contract   --mount type=volume,source="$(basename "$(pwd)")_cache",target=/code/target   --mount type=volume,source=registry_cache,target=/usr/local/cargo/registry   enigmampc/secret-contract-optimizer:1.0.9

4) secretcli tx compute store contract.wasm.gz --from <your-key> -y --gas 100000000 --gas-prices=1.0uscrt

5) secretcli query compute list-code

## Instantiation

6) INIT='{"name":"contract-marketplace","entropy":"fides", "symbol":"jh","config":{"public_token_supply":true,"public_owner":true}}'

7) CODE_ID=<code_id obtained as a result of command no. 4>

8) secretcli tx compute instantiate $CODE_ID "$INIT" --from <your-key> --label "<your-label>" -y --keyring-backend test

9) secretcli query compute list-contract-by-code CODE_ID

10) CONTRACT=<contract address obtained in previous command>

11) To mint without royalty use command:
secretcli tx compute execute $CONTRACT '{"mint_nft": {"token_id":"1","transferable":true}}' --from <your-key> --keyring-backend test

12) To mint by providing royalty:
secretcli tx compute execute $CONTRACT '{"mint_nft": {"token_id":"2","royalty_info":{"decimal_places_in_rates":2,"royalties":[{"recipient":"<address to receive royalty>","rate":2}]},"transferable":true}}' --from <your-key> --keyring-backend test

13) To set default royalty of contract:
secretcli tx compute execute $CONTRACT '{"set_royalty_info": {"royalty_info":{"decimal_places_in_rates":2,"royalties":[{"recipient":"<address to receive royalty>","rate":2}]}}}' --from <your-key> --keyring-backend test

14) Check address balance using: 
secretcli query bank balances <address>





