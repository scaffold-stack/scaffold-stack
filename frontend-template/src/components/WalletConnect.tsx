"use client";
import { createContext, useContext, useState, useEffect, ReactNode } from 'react';
import { connect, disconnect, isConnected, getLocalStorage } from '@stacks/connect';
import type { GetAddressesResult } from '@stacks/connect/dist/types/methods';


type WalletContextValue = {
  address: string | null;
  connected: boolean;
  connect: () => Promise<void>;
  disconnect: () => void;
};

const WalletContext = createContext<WalletContextValue | undefined>(undefined);

export function WalletProvider({ children }: { children: ReactNode }) {
  const [address, setAddress] = useState<string | null>(null);

  // Restore session on mount
  useEffect(() => {
    if (isConnected()) {
      const stored = getLocalStorage();
      // v8: addresses[2] is the STX address (0=BTC native, 1=BTC taproot, 2=STX)
       //@ts-ignore
      const addr = stored?.addresses?.[2]?.address ?? null;
      if (addr) setAddress(addr);
    }
  }, []);

  const handleConnect = async () => {
    const response: GetAddressesResult = await connect();
    // addresses[2] is the STX address in v8
    const addr = response.addresses[2]?.address ?? null;
    if (addr) setAddress(addr);
  };

  const handleDisconnect = () => {
    disconnect();
    setAddress(null);
  };

  return (
    <WalletContext.Provider value={{
      address,
      connected: !!address,
      connect: handleConnect,
      disconnect: handleDisconnect,
    }}>
      {children}
    </WalletContext.Provider>
  );
}

export function useWallet() {
  const ctx = useContext(WalletContext);
  if (!ctx) throw new Error('useWallet must be used within WalletProvider');
  return ctx;
}

export function WalletConnect() {
  const { address, connected, connect: connectWallet, disconnect: disconnectWallet } = useWallet();
  const [connecting, setConnecting] = useState(false);

  const onConnect = async () => {
    setConnecting(true);
    try { await connectWallet(); }
    catch (e) { console.error('[scaffold-stacks] wallet connect failed:', e); }
    finally { setConnecting(false); }
  };

  if (!connected) {
    return (
      <button
        onClick={onConnect}
        disabled={connecting}
        style={{
          display: 'flex', alignItems: 'center', gap: '8px',
          padding: '8px 16px', borderRadius: '8px',
          background: connecting ? '#065f46' : '#059669',
          color: '#fff', fontSize: '14px', fontWeight: 600,
          border: 'none', cursor: connecting ? 'not-allowed' : 'pointer',
          opacity: connecting ? 0.7 : 1, transition: 'all 0.15s',
        }}
      >
        {connecting ? 'Connecting...' : 'Connect Wallet'}
      </button>
    );
  }

  const short = address ? `${address.slice(0, 6)}…${address.slice(-4)}` : '';

  return (
    <div style={{ display: 'flex', alignItems: 'center', gap: '8px' }}>
      <div style={{
        display: 'flex', alignItems: 'center', gap: '8px',
        padding: '6px 12px', borderRadius: '8px',
        background: '#111827', border: '1px solid #1f2937',
      }}>
        <div style={{ width: '6px', height: '6px', borderRadius: '50%', background: '#34d399' }} />
        <span style={{ fontFamily: 'monospace', fontSize: '14px', color: '#34d399' }}>{short}</span>
      </div>
      <button
        onClick={disconnectWallet}
        style={{
          padding: '6px 12px', borderRadius: '8px',
          background: '#1f2937', border: '1px solid #374151',
          color: '#9ca3af', fontSize: '12px', fontWeight: 500, cursor: 'pointer',
        }}
      >
        Disconnect
      </button>
    </div>
  );
}