// Network is driven by NEXT_PUBLIC_NETWORK env var.
// stacksdapp dev --network testnet sets this automatically in frontend/.env.local

import { createNetwork, type StacksNetwork, type StacksNetworkName } from '@stacks/network';

type ScaffoldNetwork = 'devnet' | 'testnet' | 'mainnet';

function resolveNetwork(value: string | undefined): ScaffoldNetwork {
  const network = value ?? 'devnet';
  if (network === 'devnet' || network === 'testnet' || network === 'mainnet') {
    return network;
  }
  throw new Error(
    `[scaffold-stacks] Invalid NEXT_PUBLIC_NETWORK="${network}". Expected one of: devnet | testnet | mainnet.`,
  );
}

const network = resolveNetwork(process.env.NEXT_PUBLIC_NETWORK);

const nodeUrl =
  network === 'mainnet'
    ? (process.env.NEXT_PUBLIC_STACKS_NODE_URL ?? 'https://api.hiro.so')
    : network === 'testnet'
    ? (process.env.NEXT_PUBLIC_STACKS_NODE_URL ?? 'https://api.testnet.hiro.so')
    : (process.env.NEXT_PUBLIC_STACKS_NODE_URL ?? 'http://127.0.0.1:20443');

/** Stacks core RPC. In the browser this is proxied so devnet calls are not blocked by CORS. */
export function devnetCoreBaseUrl(): string {
  if (typeof window !== 'undefined') {
    return `${window.location.origin}/devnet-node`;
  }
  return nodeUrl;
}

export function getDevnetCoreNetwork(): StacksNetwork {
  const core = createNetwork('devnet');
  return {
    ...core,
    client: {
      ...core.client,
      baseUrl: devnetCoreBaseUrl(),
    },
  };
}

export const scaffoldConfig = {
  network,
  // Browser-wallet requests only understand public chain names, so devnet
  // writes use a local signer while testnet/mainnet still use @stacks/connect.
  requestNetwork: network === 'devnet' ? 'testnet' : network,
  targetNetwork: network === 'devnet' ? 'testnet' : network,
  nodeUrl,
  hiroApiKey: process.env.NEXT_PUBLIC_HIRO_API_KEY ?? '',
  explorerBaseUrl: network === 'mainnet' ? 'https://explorer.hiro.so/txid/' : 'https://explorer.hiro.so/txid/',
  explorerChainQuery: network === 'mainnet' ? '?chain=mainnet' : '?chain=testnet',
  isDevnet:  network === 'devnet',
  isTestnet: network === 'testnet',
  isMainnet: network === 'mainnet',
} as const;

/** Network object for @stacks/transactions read-only calls (optional Hiro API key). */
export function getReadOnlyNetwork(): StacksNetworkName | StacksNetwork {
  if (scaffoldConfig.isDevnet) {
    return getDevnetCoreNetwork();
  }
  const networkName: StacksNetworkName = scaffoldConfig.targetNetwork;
  if (scaffoldConfig.hiroApiKey) {
    return createNetwork(networkName, scaffoldConfig.hiroApiKey);
  }
  return networkName;
}
