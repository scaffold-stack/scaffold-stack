"use client";
import { createContext, useContext, useState, useEffect, ReactNode } from 'react';
import { connect, disconnect, isConnected, getLocalStorage } from '@stacks/connect';
import { addressAtom, isMountedAtom } from '../store/wallet';
import { useAtom, useAtomValue, useSetAtom } from 'jotai';
import { scaffoldConfig } from '../scaffold.config';
import {
  ensureDefaultBurner,
  getDevnetBurners,
  setSelectedBurner,
} from '../lib/devnet';


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

    if (scaffoldConfig.isDevnet) {
      const burner = ensureDefaultBurner();
      if (burner.address !== address) {
        setAddress(burner.address);
      }
      return;
    }

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
  const [burnerId, setBurnerId] = useState<string>('wallet_1');

  useEffect(() => {
    if (!isMounted || !scaffoldConfig.isDevnet) return;
    const burner = ensureDefaultBurner();
    setBurnerId(burner.id);
    if (burner.address !== address) {
      setAddress(burner.address);
    }
  }, [address, isMounted, setAddress]);

  const handleConnect = async () => {
    if (scaffoldConfig.isDevnet) {
      const burner = ensureDefaultBurner();
      setBurnerId(burner.id);
      setAddress(burner.address);
      return;
    }

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
    if (scaffoldConfig.isDevnet) return;
    disconnect();
    setAddress(null);
    // Optional: window.location.reload() to hard-clear all state
  };

  const handleBurnerChange = (value: string) => {
    const burner = setSelectedBurner(value);
    setBurnerId(burner.id);
    setAddress(burner.address);
  };

  // 1. Prevents SSR Flash: Render nothing or a skeleton until client-side mount
  if (!isMounted) return <div style={{ width: '140px', height: '38px' }} />;

  if (scaffoldConfig.isDevnet) {
    const burners = getDevnetBurners();
    const selected = burners.find(burner => burner.id === burnerId) ?? ensureDefaultBurner();
    const short = `${selected.address.slice(0, 6)}…${selected.address.slice(-4)}`;

    return (
      <div style={{ display: 'flex', alignItems: 'center', gap: '8px' }}>
        <select
          value={selected.id}
          onChange={e => handleBurnerChange(e.target.value)}
          style={{
            padding: '8px 12px',
            borderRadius: '8px',
            background: '#111827',
            color: '#fff',
            border: '1px solid #1f2937',
          }}
        >
          {burners.map(burner => (
            <option key={burner.id} value={burner.id}>
              {burner.label}
            </option>
          ))}
        </select>
        <div style={{
          padding: '6px 12px',
          borderRadius: '8px',
          background: '#111827',
          border: '1px solid #1f2937',
          color: '#34d399'
        }}>
          {short}
        </div>
      </div>
    );
  }

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
