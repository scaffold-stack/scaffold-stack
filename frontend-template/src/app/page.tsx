import DebugContracts from '../components/debug/DebugContracts';
import { WalletConnect } from '../components/WalletConnect';

export default function Home() {
  return (
    <main className="min-h-screen bg-gray-950 text-white">
      <header className="border-b border-gray-800 px-6 py-4 flex items-center justify-between">
        <div className="flex items-center gap-3">
          <span className="text-emerald-400 text-xl font-bold">⚡ scaffold-stacks</span>
          <NetworkBadge />
        </div>
        <WalletConnect />
      </header>
      <div className="max-w-4xl mx-auto py-8 px-4">
        <DebugContracts />
      </div>
    </main>
  );
}

function NetworkBadge() {
  const network = process.env.NEXT_PUBLIC_NETWORK ?? 'devnet';
  const colours: Record<string, string> = {
    devnet:  'bg-yellow-500/20 text-yellow-400 border-yellow-500/30',
    testnet: 'bg-blue-500/20 text-blue-400 border-blue-500/30',
    mainnet: 'bg-emerald-500/20 text-emerald-400 border-emerald-500/30',
  };
  return (
    <span className={`text-xs px-2 py-0.5 rounded border font-mono ${colours[network] ?? colours.devnet}`}>
      {network}
    </span>
  );
}