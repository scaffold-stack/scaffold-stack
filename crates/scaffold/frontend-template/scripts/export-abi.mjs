// frontend/scripts/export-abi.mjs
// Called by the scaffold-stacks CLI (parser crate) with CWD = contracts/
// Prints a JSON array of ContractAbi objects to stdout.

import { initSimnet } from "@stacks/clarinet-sdk";
import { createHash } from "crypto";
import { resolve } from "path";
import {
  readFileSync,
  writeFileSync,
  existsSync,
  unlinkSync,
  statSync,
  mkdirSync,
} from "fs";

const manifestPath = resolve("Clarinet.toml");
const simnetPlanPath = resolve("deployments/default.simnet-plan.yaml");
const cachePath = resolve(".cache/abi-export-cache.json");
const FORCE = process.env.STACKSDAPP_FORCE_ABI_EXPORT === "1";

function sha256(text) {
  return createHash("sha256").update(text).digest("hex");
}

function collectContractPaths() {
  const paths = [manifestPath];
  const clarinetRaw = readFileSync(manifestPath, "utf8");
  for (const line of clarinetRaw.split("\n")) {
    const pathMatch = line.match(/^\s*path\s*=\s*"([^"]+)"/);
    if (pathMatch) {
      paths.push(resolve(pathMatch[1]));
    }
  }
  return paths;
}

function computeFingerprint() {
  const parts = collectContractPaths()
    .filter((p) => existsSync(p))
    .map((p) => {
      const st = statSync(p);
      return `${p}:${st.mtimeMs}:${st.size}`;
    })
    .sort();
  return sha256(parts.join("\n"));
}

function readCache() {
  if (!existsSync(cachePath)) return null;
  try {
    return JSON.parse(readFileSync(cachePath, "utf8"));
  } catch {
    return null;
  }
}

function writeCache(fingerprint, abis) {
  mkdirSync(resolve(".cache"), { recursive: true });
  writeFileSync(
    cachePath,
    JSON.stringify({ fingerprint, abis, cached_at: new Date().toISOString() }, null, 2)
  );
}

const fingerprint = computeFingerprint();
if (!FORCE) {
  const cache = readCache();
  if (cache?.fingerprint === fingerprint && Array.isArray(cache.abis)) {
    process.stderr.write("Using cached ABIs (unchanged contracts)\n");
    process.stdout.write(JSON.stringify(cache.abis, null, 2) + "\n");
    process.exit(0);
  }
}

// initSimnet() can reuse a stale on-disk plan that still references renamed
// contract files. Removing the cached simnet plan forces Clarinet SDK to
// regenerate it from the current Clarinet.toml contract list.
if (existsSync(simnetPlanPath)) {
  unlinkSync(simnetPlanPath);
}

// Parse Clarinet.toml to get the exact list of user contracts.
const clarinetRaw = readFileSync(manifestPath, "utf8");
const userContracts = new Set();
for (const line of clarinetRaw.split("\n")) {
  const match = line.match(/^\[contracts\.([^\]]+)\]/);
  if (match) userContracts.add(match[1]);
}

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

process.stdout.write = realWrite;

const interfaces = simnet.getContractsInterfaces();
const abis = [];

for (const [contractId, iface] of interfaces) {
  const parts = contractId.split(".");
  const contractName = parts[parts.length - 1];

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

try {
  writeCache(fingerprint, abis);
} catch (err) {
  process.stderr.write(`[export-abi] Warning: could not write ABI cache: ${err?.message ?? err}\n`);
}

process.stdout.write(JSON.stringify(abis, null, 2) + "\n");
