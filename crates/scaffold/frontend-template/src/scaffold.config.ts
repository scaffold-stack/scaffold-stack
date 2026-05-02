// Network is driven by NEXT_PUBLIC_NETWORK env var.
// stacksdapp dev --network testnet sets this automatically in frontend/.env.local

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
    : (process.env.NEXT_PUBLIC_STACKS_NODE_URL ?? 'http://localhost:3999');

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
