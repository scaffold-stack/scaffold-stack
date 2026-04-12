// Network is driven by NEXT_PUBLIC_NETWORK env var.
// stacksdapp dev --network testnet sets this automatically in frontend/.env.local

const network = (process.env.NEXT_PUBLIC_NETWORK ?? 'devnet') as 'devnet' | 'testnet' | 'mainnet';

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
  isDevnet:  network === 'devnet',
  isTestnet: network === 'testnet',
  isMainnet: network === 'mainnet',
} as const;
