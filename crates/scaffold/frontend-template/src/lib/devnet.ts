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
import { scaffoldConfig } from '../scaffold.config';

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
  {
    id: 'wallet_4',
    label: 'wallet_4',
    mnemonic: 'board list obtain sugar hour worth raven scout denial thunder horse logic fury scorpion fold genuine phrase wealth news aim below celery when cabin',
  },
  {
    id: 'wallet_5',
    label: 'wallet_5',
    mnemonic: 'hurry aunt blame peanut heavy update captain human rice crime juice adult scale device promote vast project quiz unit note reform update climb purchase',
  },
  {
    id: 'wallet_6',
    label: 'wallet_6',
    mnemonic: 'area desk dutch sign gold cricket dawn toward giggle vibrant indoor bench warfare wagon number tiny universe sand talk dilemma pottery bone trap buddy',
  },
  {
    id: 'wallet_7',
    label: 'wallet_7',
    mnemonic: 'prevent gallery kind limb income control noise together echo rival record wedding sense uncover school version force bleak nuclear include danger skirt enact arrow',
  },
  {
    id: 'wallet_8',
    label: 'wallet_8',
    mnemonic: 'female adjust gallery certain visit token during great side clown fitness like hurt clip knife warm bench start reunion globe detail dream depend fortune',
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
    throw new Error(`Unknown burner wallet: ${burnerId}`);
  }
  if (typeof window !== 'undefined') {
    window.localStorage.setItem(STORAGE_KEY, burner.id);
  }
  return burner;
}

export function ensureDefaultBurner(): DevnetBurner {
  return getSelectedBurner() ?? setSelectedBurner('wallet_1');
}

export function getDevnetSenderAddress(): string | null {
  if (!scaffoldConfig.isDevnet) return null;
  return ensureDefaultBurner().address;
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

  const burner = ensureDefaultBurner();
  const dot = contract.lastIndexOf('.');
  if (dot < 0) {
    throw new Error(`Invalid contract identifier: ${contract}`);
  }

  const transaction = await makeContractCall({
    contractAddress: contract.slice(0, dot),
    contractName: contract.slice(dot + 1),
    functionName,
    functionArgs,
    postConditions,
    postConditionMode: PostConditionMode.Allow,
    senderKey: burner.privateKey,
    network: 'devnet',
    validateWithAbi: true,
  });

  const result: any = await broadcastTransaction({ transaction, network: 'devnet' });
  if (!result?.txid) {
    throw new Error(result?.reason || 'Devnet transaction failed');
  }
  return result;
}
