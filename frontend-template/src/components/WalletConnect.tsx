"use client";
import { createContext, useContext, useState, useEffect, ReactNode } from 'react';
import { connect, disconnect, isConnected, getLocalStorage } from '@stacks/connect';
import { addressAtom, isMountedAtom } from '../store/wallet';
import { useAtom, useAtomValue, useSetAtom } from 'jotai';


type WalletContextValue = {
  address: string | null;
  connected: boolean;
  connect: () => Promise<void>;
  disconnect: () => void;
};

const WalletContext = createContext<WalletContextValue | undefined>(undefined);

export function WalletProvider({ children }: { children: React.ReactNode }) {
  const [address, setAddress] = useAtom(addressAtom);
  const [, setMounted] = useAtom(isMountedAtom);

  useEffect(() => {
    setMounted(true);
    
    // If Stacks says we are connected but Jotai is empty (e.g. first load)
    if (isConnected()) {
      const stored = getLocalStorage();
      //@ts-ignore
      const addr = stored?.addresses?.[2]?.address ?? null;
      if (addr && addr !== address) {
        setAddress(addr);
      }
    }
  }, [address, setAddress, setMounted]);

  return <>{children}</>;
}


export function useWallet() {
  const ctx = useContext(WalletContext);
  if (!ctx) throw new Error('useWallet must be used within WalletProvider');
  return ctx;
}


export function WalletConnect() {
  const address = useAtomValue(addressAtom);
  const isMounted = useAtomValue(isMountedAtom);
  const setAddress = useSetAtom(addressAtom);
  const [connecting, setConnecting] = useState(false);

  const handleConnect = async () => {
    setConnecting(true);
    try {
      const response = await connect();
      const addr = response.addresses[2]?.address ?? null;
      setAddress(addr);
    } catch (e) {
      console.error('[scaffold-stacks] connection failed:', e);
    } finally {
      setConnecting(false);
    }
  };

  const handleDisconnect = () => {
    disconnect();
    setAddress(null);
    // Optional: window.location.reload() to hard-clear all state
  };

  // 1. Prevents SSR Flash: Render nothing or a skeleton until client-side mount
  if (!isMounted) return <div style={{ width: '140px', height: '38px' }} />;

  // 2. Disconnected UI
  if (!address) {
    return (
      <button
        onClick={handleConnect}
        disabled={connecting}
        style={{
          padding: '8px 16px', borderRadius: '8px',
          background: '#059669', color: '#fff', fontWeight: 600,
          border: 'none', cursor: 'pointer',
        }}
      >
        {connecting ? 'Connecting...' : 'Connect Wallet'}
      </button>
    );
  }

  // 3. Connected UI
  const short = `${address.slice(0, 6)}…${address.slice(-4)}`;
  return (
    <div style={{ display: 'flex', alignItems: 'center', gap: '8px' }}>
      <div style={{
        padding: '6px 12px', borderRadius: '8px',
        background: '#111827', border: '1px solid #1f2937', color: '#34d399'
      }}>
        {short}
      </div>
      <button 
        onClick={handleDisconnect}
        style={{ padding: '6px 12px', color: '#9ca3af', cursor: 'pointer', background: 'transparent', border: 'none' }}
      >
        Disconnect
      </button>
    </div>
  );
}
