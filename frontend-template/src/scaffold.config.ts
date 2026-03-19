// Network is driven by NEXT_PUBLIC_NETWORK env var.
// stacksdapp dev --network testnet sets this automatically in frontend/.env.local

const network = (process.env.NEXT_PUBLIC_NETWORK ?? 'devnet') as 'devnet' | 'testnet' | 'mainnet';

export const scaffoldConfig = {
  // targetNetwork: string used by request() and fetchCallReadOnlyFunction() in v8/v7
  targetNetwork: network === 'devnet' ? 'testnet' : network,

  // Node URL for direct API calls
  nodeUrl:
    network === 'mainnet'
      ? (process.env.NEXT_PUBLIC_STACKS_NODE_URL ?? 'https://api.hiro.so')
      : network === 'testnet'
      ? (process.env.NEXT_PUBLIC_STACKS_NODE_URL ?? 'https://api.testnet.hiro.so')
      : (process.env.NEXT_PUBLIC_STACKS_NODE_URL ?? 'http://localhost:3999'),

  isDevnet:  network === 'devnet',
  isTestnet: network === 'testnet',
  isMainnet: network === 'mainnet',
} as const;