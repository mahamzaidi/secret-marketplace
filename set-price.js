const {
    EnigmaUtils,
    Secp256k1Pen,
    SigningCosmWasmClient,
    pubkeyToAddress,
    encodeSecp256k1Pubkey,
} = require("secretjs");

// Requiring the dotenv package in this way
// lets us use environment variables defined in .env
require("dotenv").config();

const customFees = {
    upload: {
        amount: [{ amount: "2000000", denom: "uscrt" }],
        gas: "2000000",
    },
    init: {
        amount: [{ amount: "500000", denom: "uscrt" }],
        gas: "500000",
    },
    exec: {
        amount: [{ amount: "500000", denom: "uscrt" }],
        gas: "500000",
    },
    send: {
        amount: [{ amount: "80000", denom: "uscrt" }],
        gas: "80000",
    },
};

const main = async () => {
    const httpUrl = process.env.SECRET_REST_URL;

    // Use the mnemonic created in step #2 of the Secret Pathway
    const mnemonic = process.env.MNEMONIC;

    // A pen is the most basic tool you can think of for signing.
    // This wraps a single keypair and allows for signing.
    const signingPen = await Secp256k1Pen.fromMnemonic(mnemonic).catch((err) => {
        throw new Error(`Could not get signing pen: ${err}`);
    });

    // Get the public key
    const pubkey = encodeSecp256k1Pubkey(signingPen.pubkey);

    // get the wallet address
    const accAddress = pubkeyToAddress(pubkey, "secret");

    // initialize client
    const txEncryptionSeed = EnigmaUtils.GenerateNewSeed();

    const client = new SigningCosmWasmClient(
        httpUrl,
        accAddress,
        (signBytes) => signingPen.sign(signBytes),
        txEncryptionSeed,
        customFees
    );
    console.log(`Wallet address=${accAddress}`);

    const handleMsg = {
        set_price: {
            token_id: "5",
            price: 2,
        },
    };

    console.log("Setting price");
    const response = await client
        .execute(process.env.SECRET_NFT_CONTRACT, handleMsg)
        .catch((err) => {
            throw new Error(`Could not execute contract: ${err}`);
        });
    console.log("response: ", response);
};

main().catch((err) => {
    console.error(err);
});
