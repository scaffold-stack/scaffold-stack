// frontend/scripts/export-abi.mjs
// Called by the scaffold-stacks CLI (parser crate) with CWD = contracts/
// Prints a JSON array of ContractAbi objects to stdout.

import { initSimnet } from "@stacks/clarinet-sdk";
import { resolve } from "path";
import { readFileSync, existsSync, unlinkSync } from "fs";

const manifestPath = resolve("Clarinet.toml");
const simnetPlanPath = resolve("deployments/default.simnet-plan.yaml");

// initSimnet() can reuse a stale on-disk plan that still references renamed
// contract files. Removing the cached simnet plan forces Clarinet SDK to
// regenerate it from the current Clarinet.toml contract list.
if (existsSync(simnetPlanPath)) {
  unlinkSync(simnetPlanPath);
}

// Parse Clarinet.toml to get the exact list of user contracts.
// Only these appear in output — boot contracts (pox, costs, etc.) are excluded.
const clarinetRaw = readFileSync(manifestPath, "utf8");
const userContracts = new Set();
for (const line of clarinetRaw.split("\n")) {
  const match = line.match(/^\[contracts\.([^\]]+)\]/);
  if (match) userContracts.add(match[1]);
}

// initSimnet() writes "Updated deployment plan file" (and similar status lines)
// to stdout, which corrupts the JSON output. Redirect stdout to stderr for the
// duration of the call so only our JSON ends up on stdout.
const realWrite = process.stdout.write.bind(process.stdout);
process.stdout.write = (...args) => process.stderr.write(...args);

let simnet;
try {
  simnet = await initSimnet(manifestPath);
} catch (err) {
  process.stdout.write = realWrite;
  process.stderr.write(
    `initSimnet failed: ${err?.message ?? err}\n` +
    `Ensure CWD is the contracts/ directory and settings/ contains valid *.toml files.\n`
  );
  process.exit(1);
}

// Restore stdout before we write our JSON
process.stdout.write = realWrite;

const interfaces = simnet.getContractsInterfaces();
const abis = [];

for (const [contractId, iface] of interfaces) {
  const parts = contractId.split(".");
  const contractName = parts[parts.length - 1];

  // Only include contracts explicitly listed in Clarinet.toml
  if (!userContracts.has(contractName)) {
    continue;
  }

  abis.push({
    contract_id: contractId,
    contract_name: contractName,
    functions: (iface.functions ?? []).map((fn) => ({
      name: fn.name,
      access: fn.access,
      args: (fn.args ?? []).map((a) => ({ name: a.name, type: a.type })),
      outputs: fn.outputs?.type ?? "none",
    })),
    variables: (iface.variables ?? []).map((v) => ({
      name: v.name,
      access: v.access,
      type: v.type,
    })),
    maps: (iface.maps ?? []).map((m) => ({
      name: m.name,
      key: m.key,
      value: m.value,
    })),
    fungible_tokens: (iface.fungible_tokens ?? []).map((ft) =>
      typeof ft === "string" ? ft : ft.name
    ),
    non_fungible_tokens: (iface.non_fungible_tokens ?? []).map((nft) => ({
      name: typeof nft === "string" ? nft : nft.name,
      type: nft.type ?? "none",
    })),
  });
}

process.stdout.write(JSON.stringify(abis, null, 2) + "\n");
