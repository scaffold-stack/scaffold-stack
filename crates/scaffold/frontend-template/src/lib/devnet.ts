"use client";

import { HDKey } from '@scure/bip32';
import { mnemonicToSeedSync } from '@scure/bip39';
import { bytesToHex } from '@stacks/common';
import {
  ClarityValue,
  PostConditionMode,
  broadcastTransaction,
  makeContractCall,
  privateKeyToAddress,
} from '@stacks/transactions';
import { getDevnetCoreNetwork, scaffoldConfig, devnetCoreBaseUrl } from '../scaffold.config';

const DERIVATION_PATH = "m/44'/5757'/0'/0/0";
const STORAGE_KEY = 'stacksdapp.devnet.burner';

type BurnerSeed = {
  id: string;
  label: string;
  mnemonic: string;
};

export type DevnetBurner = {
  id: string;
  label: string;
  address: string;
  privateKey: string;
};

const BURNER_SEEDS: BurnerSeed[] = [
  {
    id: 'deployer',
    label: 'deployer',
    mnemonic: 'twice kind fence tip hidden tilt action fragile skin nothing glory cousin green tomorrow spring wrist shed math olympic multiply hip blue scout claw',
  },
  {
    id: 'wallet_1',
    label: 'wallet_1',
    mnemonic: 'sell invite acquire kitten bamboo drastic jelly vivid peace spawn twice guilt pave pen trash pretty park cube fragile unaware remain midnight betray rebuild',
  },
  {
    id: 'wallet_2',
    label: 'wallet_2',
    mnemonic: 'hold excess usual excess ring elephant install account glad dry fragile donkey gaze humble truck breeze nation gasp vacuum limb head keep delay hospital',
  },
  {
    id: 'wallet_3',
    label: 'wallet_3',
    mnemonic: 'cycle puppy glare enroll cost improve round trend wrist mushroom scorpion tower claim oppose clever elephant dinosaur eight problem before frozen dune wagon high',
  },
];

let cachedBurners: DevnetBurner[] | null = null;

function deriveBurner(seed: BurnerSeed): DevnetBurner {
  const root = HDKey.fromMasterSeed(mnemonicToSeedSync(seed.mnemonic));
  const child = root.derive(DERIVATION_PATH);
  if (!child.privateKey) {
    throw new Error(`Failed to derive private key for ${seed.id}`);
  }
  const privateKey = `${bytesToHex(child.privateKey)}01`;
  return {
    id: seed.id,
    label: seed.label,
    address: privateKeyToAddress(privateKey, 'testnet'),
    privateKey,
  };
}

export function getDevnetBurners(): DevnetBurner[] {
  if (!cachedBurners) {
    cachedBurners = BURNER_SEEDS.map(deriveBurner);
  }
  return cachedBurners;
}

export function getSelectedBurner(): DevnetBurner | null {
  if (typeof window === 'undefined') return null;
  const burnerId = window.localStorage.getItem(STORAGE_KEY);
  if (!burnerId) return null;
  return getDevnetBurners().find(burner => burner.id === burnerId) ?? null;
}

export function setSelectedBurner(burnerId: string): DevnetBurner {
  const burner = getDevnetBurners().find(item => item.id === burnerId);
  if (!burner) {
    throw new Error(`Unknown devnet wallet: ${burnerId}`);
  }
  if (typeof window !== 'undefined') {
    window.localStorage.setItem(STORAGE_KEY, burner.id);
  }
  return burner;
}

export function clearSelectedBurner() {
  if (typeof window !== 'undefined') {
    window.localStorage.removeItem(STORAGE_KEY);
  }
}

export function getDevnetSenderAddress(): string | null {
  if (!scaffoldConfig.isDevnet) return null;
  return getSelectedBurner()?.address ?? null;
}

export function parseMicrostacks(raw: unknown): bigint | null {
  if (typeof raw === 'bigint') return raw;
  if (typeof raw === 'number' && Number.isFinite(raw)) return BigInt(Math.trunc(raw));
  if (typeof raw !== 'string' || raw.length === 0) return null;
  try {
    if (raw.startsWith('0x') || raw.startsWith('0X')) return BigInt(raw);
    if (/^\d+$/.test(raw)) return BigInt(raw);
  } catch {
    return null;
  }
  return null;
}

export function formatStx(micro: bigint): string {
  const whole = micro / 1_000_000n;
  const frac = micro % 1_000_000n;
  if (frac === 0n) return `${whole.toString()} STX`;
  const fracText = frac.toString().padStart(6, '0').replace(/0+$/, '');
  return `${whole.toString()}.${fracText} STX`;
}

export async function fetchDevnetBalance(address: string): Promise<string | null> {
  try {
    const response = await fetch(`${devnetCoreBaseUrl()}/v2/accounts/${address}?proof=0`, {
      cache: 'no-store',
    });
    if (!response.ok) return null;
    const body = await response.json();
    const micro = parseMicrostacks(body?.balance);
    return micro == null ? null : formatStx(micro);
  } catch {
    return null;
  }
}

export type DevnetTxPoll =
  | { state: 'pending' }
  | { state: 'success' }
  | { state: 'abort'; message: string }
  | { state: 'error'; message: string };

/** Nonce the broadcast used. Core has no tx-index by default, so inclusion is "account nonce moved past this". */
const inflightNonces = new Map<string, { sender: string; nonce: bigint }>();

export function trackDevnetTx(txid: string, sender: string, nonce: bigint) {
  inflightNonces.set(txid, { sender, nonce });
}

export function devnetInclusionStatus(accountNonce: bigint, txNonce: bigint): DevnetTxPoll {
  return accountNonce > txNonce ? { state: 'success' } : { state: 'pending' };
}

function interpretTxBody(body: unknown): DevnetTxPoll | null {
  if (!body || typeof body !== 'object') return null;
  const record = body as {
    tx_status?: unknown;
    status?: unknown;
    tx_result?: { repr?: unknown };
    error?: unknown;
    reason?: unknown;
  };
  const status = String(record.tx_status ?? record.status ?? '').toLowerCase();
  if (status.includes('success')) return { state: 'success' };
  if (status.includes('abort')) {
    const repr = record.tx_result?.repr;
    return {
      state: 'abort',
      message: typeof repr === 'string' && repr.length > 0 ? repr : 'Transaction aborted.',
    };
  }
  if (status.includes('pending') || status.includes('mempool')) return { state: 'pending' };
  if (record.error || record.reason) {
    const detail = record.reason ?? record.error;
    return {
      state: 'error',
      message: typeof detail === 'string' ? detail : 'Devnet transaction failed.',
    };
  }
  return null;
}

async function fetchAccountNonce(address: string): Promise<bigint | null> {
  try {
    const response = await fetch(`${devnetCoreBaseUrl()}/v2/accounts/${address}?proof=0`, {
      cache: 'no-store',
    });
    if (!response.ok) return null;
    const body = await response.json();
    if (typeof body?.nonce === 'number' || typeof body?.nonce === 'string') {
      return BigInt(body.nonce);
    }
  } catch {
    return null;
  }
  return null;
}

/** Poll until the devnet transaction is included. Stacks core keeps a mined tx on the unconfirmed route, so inclusion is the sender nonce moving past the tx nonce. */
export async function readDevnetTxStatus(txid: string): Promise<DevnetTxPoll> {
  const base = devnetCoreBaseUrl();
  for (const path of [`/extended/v1/tx/${txid}`, `/v3/transaction/${txid}`]) {
    try {
      const response = await fetch(`${base}${path}`, { cache: 'no-store' });
      if (!response.ok) continue;
      const parsed = interpretTxBody(await response.json().catch(() => null));
      if (parsed && parsed.state !== 'pending') return parsed;
    } catch {
      // Core is still coming up, or the proxy is not ready.
    }
  }

  const tracked = inflightNonces.get(txid);
  if (tracked) {
    const nextNonce = await fetchAccountNonce(tracked.sender);
    if (nextNonce != null) {
      const outcome = devnetInclusionStatus(nextNonce, tracked.nonce);
      if (outcome.state === 'success') inflightNonces.delete(txid);
      return outcome;
    }
  }
  return { state: 'pending' };
}

export async function callDevnetContract({
  contract,
  functionName,
  functionArgs = [],
  postConditions = [],
}: {
  contract: string;
  functionName: string;
  functionArgs?: ClarityValue[];
  postConditions?: any[];
}): Promise<any> {
  if (!scaffoldConfig.isDevnet) {
    throw new Error('Devnet signer requested outside devnet mode');
  }

  const burner = getSelectedBurner();
  if (!burner) {
    throw new Error('Choose a devnet wallet first.');
  }
  const dot = contract.lastIndexOf('.');
  if (dot < 0) {
    throw new Error(`Invalid contract identifier: ${contract}`);
  }

  const network = getDevnetCoreNetwork();
  const transaction = await makeContractCall({
    contractAddress: contract.slice(0, dot),
    contractName: contract.slice(dot + 1),
    functionName,
    functionArgs,
    postConditions,
    // Match hosted networks: Deny only when the caller passed post-conditions.
    postConditionMode: postConditions.length > 0 ? PostConditionMode.Deny : PostConditionMode.Allow,
    senderKey: burner.privateKey,
    network,
    validateWithAbi: false,
  });

  const result: any = await broadcastTransaction({ transaction, network });
  if (!result?.txid || result?.error || result?.reason) {
    const detail = result?.reason || result?.error || 'Devnet transaction failed';
    throw new Error(typeof detail === 'string' ? detail : JSON.stringify(detail));
  }
  const spentNonce = (transaction as { auth?: { spendingCondition?: { nonce?: bigint | number } } })
    .auth?.spendingCondition?.nonce;
  if (spentNonce != null) {
    trackDevnetTx(result.txid, burner.address, BigInt(spentNonce));
  }
  return result;
}
