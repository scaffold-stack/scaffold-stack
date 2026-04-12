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

function getStoredStxAddress() {
  const stored = getLocalStorage();
  if (!stored) return null;

  return (
    stored.addresses?.stx?.find(entry => entry.address.startsWith('S'))?.address ??
    stored.addresses?.stx?.[0]?.address ??
    null
  );
}

function getResponseStxAddress(addresses: Array<{ address: string; symbol?: string }>) {
  return (
    addresses.find(entry => entry.symbol === 'STX')?.address ??
    addresses.find(entry => entry.address.startsWith('S'))?.address ??
    addresses[0]?.address ??
    null
  );
}

export function WalletProvider({ children }: { children: React.ReactNode }) {
  const [, setAddress] = useAtom(addressAtom);
  const [, setMounted] = useAtom(isMountedAtom);

  useEffect(() => {
    const syncWalletState = () => {
      if (!isConnected()) {
        setAddress(null);
        return;
      }

      setAddress(getStoredStxAddress());
    };

    setMounted(true);
    syncWalletState();

    const handleVisibilityChange = () => {
      if (document.visibilityState === 'visible') {
        syncWalletState();
      }
    };

    window.addEventListener('focus', syncWalletState);
    window.addEventListener('storage', syncWalletState);
    document.addEventListener('visibilitychange', handleVisibilityChange);

    return () => {
      window.removeEventListener('focus', syncWalletState);
      window.removeEventListener('storage', syncWalletState);
      document.removeEventListener('visibilitychange', handleVisibilityChange);
    };
  }, [setAddress, setMounted]);

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
      const addr = getResponseStxAddress(response.addresses);
      setAddress(addr);
    } catch (e) {
      console.error('[scaffold-stacks] connection failed:', e);
    } finally {
      setConnecting(false);
    }
  };

  const handleDisconnect = (e: React.MouseEvent<HTMLButtonElement>) => {
    e.preventDefault();
    disconnect();
    setAddress(null);
  };

  // 1. Prevents SSR Flash: Render nothing or a skeleton until client-side mount
  if (!isMounted) return <div style={{ width: '140px', height: '38px' }} />;

  // 2. Disconnected UI
  if (!address) {
    return (
      <button
        onClick={handleConnect}
        disabled={connecting}
        className='bg-[#434242] w-[135px] h-[40px] rounded-[40px] border-[1px] border-[#1F1E1F] text-[12px] text-[#F4F3EF] font-mono leading-[100%]'
      >
        {connecting ? 'Connecting...' : 'Connect Wallet'}
      </button>
    );
  }

  // 3. Connected UI
  const short = `${address.slice(0, 6)}…${address.slice(-4)}`;
  return (
    <div style={{ display: 'flex', alignItems: 'center', gap: '8px' }}>
      <div className='bg-[#434242] w-[135px] h-[40px] rounded-[40px] border-[1px] border-[#1F1E1F] text-[12px] text-[#F4F3EF] font-mono leading-[100%] flex items-center justify-center'>
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
