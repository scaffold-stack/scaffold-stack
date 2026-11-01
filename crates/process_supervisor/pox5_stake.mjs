import { createRequire } from "node:module";
import path from "node:path";

const frontend = process.env.STACKSDAPP_FRONTEND ?? path.join(process.cwd(), "frontend");
const require = createRequire(path.join(frontend, "package.json"));
const { mnemonicToSeedSync } = require("@scure/bip39");
const { HDKey } = require("@scure/bip32");
const { bytesToHex } = require("@stacks/common");
const { STACKS_DEVNET } = require("@stacks/network");
const {
  AnchorMode,
  Cl,
  ClarityVersion,
  PostConditionMode,
  broadcastTransaction,
  getAddressFromPrivateKey,
  makeContractCall,
  makeContractDeploy,
  privateKeyToPublic,
  signStructuredData,
} = require("@stacks/transactions");

const SIGNER_KEY =
  "7287ba251d44a4d3fd9276c88ce34c5c52a038955511cccaf77e61068649c17801";
const SOURCE = `(impl-trait 'ST000000000000000000002AMW42H.pox-5.signer-manager-trait)
(use-trait signer-manager-trait 'ST000000000000000000002AMW42H.pox-5.signer-manager-trait)

(define-public (validate-stake!
        (staker principal)
        (first-index uint)
        (num-indexes uint)
        (amount-ustx uint)
        (amount-sats uint)
        (is-bond bool)
        (signer-calldata (optional (buff 500)))
    )
    (ok true)
)

(define-public (checkpoint-staker
        (staker principal)
        (first-index uint)
        (num-indexes uint)
        (is-bond bool)
    )
    (ok true)
)

(define-public (register-self
        (signer-manager <signer-manager-trait>)
        (signer-key (buff 33))
        (auth-id uint)
        (signer-sig (buff 65))
    )
    (as-contract? ()
        (try! (contract-call? 'ST000000000000000000002AMW42H.pox-5 grant-signer-key
            signer-key current-contract auth-id signer-sig
        ))
        (try! (contract-call? 'ST000000000000000000002AMW42H.pox-5 register-signer
            signer-manager signer-key
        ))
    )
)
`;

const WALLETS = [
  ["sell invite acquire kitten bamboo drastic jelly vivid peace spawn twice guilt pave pen trash pretty park cube fragile unaware remain midnight betray rebuild", 0],
  ["hold excess usual excess ring elephant install account glad dry fragile donkey gaze humble truck breeze nation gasp vacuum limb head keep delay hospital", 1],
  ["cycle puppy glare enroll cost improve round trend wrist mushroom scorpion tower claim oppose clever elephant dinosaur eight problem before frozen dune wagon high", 2],
];

const network = {
  ...STACKS_DEVNET,
  client: { baseUrl: "http://127.0.0.1:20443" },
};

function privateKey(mnemonic) {
  const seed = mnemonicToSeedSync(mnemonic);
  const child = HDKey.fromMasterSeed(seed).derive("m/44'/5757'/0'/0/0");
  return `${bytesToHex(child.privateKey)}01`;
}

async function info() {
  const response = await fetch("http://127.0.0.1:20443/v2/info");
  return response.json();
}

async function nonceOf(address) {
  const response = await fetch(
    `http://127.0.0.1:20443/v2/accounts/${address}?proof=0`,
  );
  const body = await response.json();
  return Number(body.nonce);
}

async function contractExists(address, name) {
  const response = await fetch(
    `http://127.0.0.1:20443/v2/contracts/source/${address}/${name}`,
  );
  return response.ok;
}

async function broadcast(tx, label) {
  const result = await broadcastTransaction({ transaction: tx, network });
  console.log(label, result);
  if (result.error) throw new Error(`${label}: ${result.reason ?? result.error}`);
  return result.txid;
}

function grantSignature(contractAddress, contractName, authId) {
  const domain = Cl.tuple({
    name: Cl.stringAscii("pox-5-signer"),
    version: Cl.stringAscii("1.0.0"),
    "chain-id": Cl.uint(2147483648),
  });
  const message = Cl.tuple({
    topic: Cl.stringAscii("grant-authorization"),
    "signer-manager": Cl.contractPrincipal(contractAddress, contractName),
    "auth-id": Cl.uint(authId),
  });
  return signStructuredData({ message, domain, privateKey: SIGNER_KEY });
}

function accounts() {
  return WALLETS.map(([mnemonic, index]) => {
    const key = privateKey(mnemonic);
    const address = getAddressFromPrivateKey(key, "devnet");
    return { key, address, index, name: `pox5-signer-${index}` };
  });
}

async function waitFor(label, predicate) {
  for (let attempt = 0; attempt < 40; attempt++) {
    if (await predicate()) return;
    await new Promise((resolve) => setTimeout(resolve, 5000));
  }
  throw new Error(`timed out waiting for ${label}`);
}

async function main() {
  const derived = accounts();
  if (process.argv.includes("--check")) {
    for (const account of derived) console.log(account.address);
    console.log("signer", privateKeyToPublic(SIGNER_KEY));
    return;
  }

  const chain = await info();
  if (chain.burn_block_height >= 175) {
    throw new Error(`too late, burn ${chain.burn_block_height} is in the prepare phase`);
  }
  const pox = await (await fetch("http://127.0.0.1:20443/v2/pox")).json();
  const amount = BigInt(pox.next_cycle.min_threshold_ustx) * 2n;
  const signerPub = privateKeyToPublic(SIGNER_KEY);

  for (const account of derived) {
    if (await contractExists(account.address, account.name)) continue;
    const deploy = await makeContractDeploy({
      contractName: account.name,
      codeBody: SOURCE,
      clarityVersion: ClarityVersion.Clarity6,
      senderKey: account.key,
      network,
      anchorMode: AnchorMode.Any,
      postConditionMode: PostConditionMode.Allow,
      fee: 200000n,
      nonce: await nonceOf(account.address),
    });
    await broadcast(deploy, `deploy ${account.address}.${account.name}`);
  }
  await waitFor("signer contracts", async () => {
    for (const account of derived) {
      if (!(await contractExists(account.address, account.name))) return false;
    }
    return true;
  });

  for (const account of derived) {
    const sig = grantSignature(account.address, account.name, account.index);
    const register = await makeContractCall({
      contractAddress: account.address,
      contractName: account.name,
      functionName: "register-self",
      functionArgs: [
        Cl.contractPrincipal(account.address, account.name),
        Cl.bufferFromHex(signerPub),
        Cl.uint(account.index),
        Cl.bufferFromHex(sig),
      ],
      senderKey: account.key,
      network,
      anchorMode: AnchorMode.Any,
      postConditionMode: PostConditionMode.Allow,
      fee: 200000n,
      nonce: await nonceOf(account.address),
    });
    await broadcast(register, `register ${account.name}`);
  }
  const nonceAfterRegister = new Map();
  for (const account of derived) {
    nonceAfterRegister.set(account.address, (await nonceOf(account.address)) + 1);
  }
  await waitFor("signer registration", async () => {
    for (const account of derived) {
      if ((await nonceOf(account.address)) < nonceAfterRegister.get(account.address)) return false;
    }
    return true;
  });

  const burn = (await info()).burn_block_height;
  for (const account of derived) {
    const stake = await makeContractCall({
      contractAddress: "ST000000000000000000002AMW42H",
      contractName: "pox-5",
      functionName: "stake",
      functionArgs: [
        Cl.contractPrincipal(account.address, account.name),
        Cl.uint(amount),
        Cl.uint(10),
        Cl.uint(Math.max(burn - 1, 0)),
        Cl.none(),
      ],
      senderKey: account.key,
      network,
      anchorMode: AnchorMode.Any,
      postConditionMode: PostConditionMode.Allow,
      fee: 200000n,
      nonce: await nonceOf(account.address),
    });
    await broadcast(stake, `stake ${account.address}`);
  }
}

main().catch((error) => {
  console.error(error);
  process.exit(1);
});
