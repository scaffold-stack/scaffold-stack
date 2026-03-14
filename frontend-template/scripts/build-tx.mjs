// frontend/scripts/build-tx.mjs
// Called by the deployer crate via stdin/stdout.
// Input:  JSON on stdin with { mode, privateKey, contractName, source, nonce, network }
// Output: hex-encoded transaction bytes on stdout (mode=deploy)
//         STX address string on stdout (mode=address)

import {
  makeContractDeploy,
  broadcastTransaction,
  AnchorMode,
  PostConditionMode,
  getAddressFromPrivateKey,
  TransactionVersion,
} from "@stacks/transactions";
import { StacksMainnet, StacksTestnet, StacksDevnet } from "@stacks/network";

const chunks = [];
for await (const chunk of process.stdin) chunks.push(chunk);
const input = JSON.parse(Buffer.concat(chunks).toString());

const { mode, privateKey, contractName, source, nonce, network } = input;

function getNetwork(name) {
  switch (name) {
    case "mainnet": return new StacksMainnet();
    case "testnet": return new StacksTestnet();
    default:        return new StacksDevnet();
  }
}

function getTxVersion(name) {
  return name === "mainnet"
    ? TransactionVersion.Mainnet
    : TransactionVersion.Testnet;
}

if (mode === "address") {
  const version = getTxVersion(network);
  const address = getAddressFromPrivateKey(privateKey, version);
  process.stdout.write(address + "\n");
  process.exit(0);
}

// mode === "deploy" (default)
try {
  const tx = await makeContractDeploy({
    contractName,
    codeBody: source,
    senderKey: privateKey,
    nonce: BigInt(nonce),
    network: getNetwork(network),
    anchorMode: AnchorMode.Any,
    postConditionMode: PostConditionMode.Allow,
    fee: BigInt(10_000), // 0.01 STX — sensible default; node will reject if too low
  });

  // Serialize to hex
  const { serializeTransaction } = await import("@stacks/transactions");
  const bytes = serializeTransaction(tx);
  const hex = Buffer.from(bytes).toString("hex");
  process.stdout.write(hex + "\n");
} catch (err) {
  process.stderr.write(`build-tx error: ${err?.message ?? err}\n`);
  process.exit(1);
}
