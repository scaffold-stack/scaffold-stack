"use client";
import { useState, useEffect, type ReactNode } from 'react';
import { connect, disconnect, isConnected, getLocalStorage } from '@stacks/connect';
import { addressAtom, isMountedAtom } from '../store/wallet';
import { useAtom, useAtomValue, useSetAtom } from 'jotai';
import { scaffoldConfig } from '../scaffold.config';
import {
  clearSelectedBurner,
  fetchDevnetBalance,
  getDevnetBurners,
  getSelectedBurner,
  setSelectedBurner,
} from '../lib/devnet';

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

/** Syncs Leather/Xverse connection state into Jotai atoms for the app. */
export function WalletProvider({ children }: { children: ReactNode }) {
  const [, setAddress] = useAtom(addressAtom);
  const [, setMounted] = useAtom(isMountedAtom);

  useEffect(() => {
    const syncWalletState = () => {
      if (scaffoldConfig.isDevnet) {
        setAddress(getSelectedBurner()?.address ?? null);
        return;
      }
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

export function WalletConnect() {
  if (scaffoldConfig.isDevnet) return <DevnetWalletChooser />;
  return <HostedWalletConnect />;
}

const walletButtonClass =
  'bg-[#434242] w-[135px] h-[40px] rounded-[40px] border-[1px] border-[#1F1E1F] text-[12px] text-[#F4F3EF] font-mono leading-[100%]';

function DevnetWalletChooser() {
  const address = useAtomValue(addressAtom);
  const isMounted = useAtomValue(isMountedAtom);
  const setAddress = useSetAtom(addressAtom);
  const [open, setOpen] = useState(false);
  const [balances, setBalances] = useState<Record<string, string | null>>({});
  const wallets = getDevnetBurners().map(({ id, label, address: stx }) => ({
    id,
    label,
    address: stx,
  }));

  useEffect(() => {
    if (!open) return;
    let cancelled = false;
    const accounts = getDevnetBurners();
    void Promise.all(
      accounts.map(async wallet => {
        const balance = await fetchDevnetBalance(wallet.address);
        return [wallet.id, balance] as const;
      }),
    ).then(rows => {
      if (cancelled) return;
      setBalances(Object.fromEntries(rows));
    });
    const onKey = (event: KeyboardEvent) => {
      if (event.key === 'Escape') setOpen(false);
    };
    window.addEventListener('keydown', onKey);
    return () => {
      cancelled = true;
      window.removeEventListener('keydown', onKey);
    };
  }, [open]);

  if (!isMounted) return <div style={{ width: '140px', height: '38px' }} />;

  const choose = (id: string) => {
    const burner = setSelectedBurner(id);
    setAddress(burner.address);
    setOpen(false);
  };

  const disconnectWallet = (event: React.MouseEvent<HTMLButtonElement>) => {
    event.preventDefault();
    clearSelectedBurner();
    setAddress(null);
  };

  const short = address ? `${address.slice(0, 6)}…${address.slice(-4)}` : null;

  return (
    <div style={{ display: 'flex', alignItems: 'center', gap: '8px' }}>
      <button
        type="button"
        onClick={() => setOpen(true)}
        className={short ? `${walletButtonClass} flex items-center justify-center` : walletButtonClass}
      >
        {short ?? 'Choose Wallet'}
      </button>
      {short && (
        <button
          type="button"
          onClick={disconnectWallet}
          style={{ padding: '6px 12px', color: '#9ca3af', cursor: 'pointer', background: 'transparent', border: 'none' }}
        >
          Disconnect
        </button>
      )}
      {open && (
        <div
          role="dialog"
          aria-modal="true"
          aria-label="Choose devnet wallet"
          onClick={() => setOpen(false)}
          style={{
            position: 'fixed',
            inset: 0,
            zIndex: 50,
            background: 'rgba(0,0,0,0.55)',
            display: 'flex',
            justifyContent: 'center',
            paddingTop: '120px',
          }}
        >
          <div
            onClick={event => event.stopPropagation()}
            style={{
              width: '320px',
              maxHeight: '420px',
              overflowY: 'auto',
              borderRadius: '16px',
              border: '1px solid #434242',
              background: '#1F1E1F',
            }}
          >
            <p style={{ margin: 0, padding: '14px 16px 8px', fontFamily: 'ui-monospace, monospace', fontSize: '12px', color: '#8F8D8E' }}>
              Funded devnet wallets
            </p>
            {wallets.map(wallet => {
              const selected = wallet.address === address;
              return (
                <button
                  key={wallet.id}
                  type="button"
                  onClick={() => choose(wallet.id)}
                  style={{
                    display: 'block',
                    width: '100%',
                    textAlign: 'left',
                    padding: '12px 16px',
                    border: 'none',
                    borderTop: '1px solid #2E2D2E',
                    background: selected ? '#434242' : 'transparent',
                    cursor: 'pointer',
                    fontFamily: 'ui-monospace, monospace',
                  }}
                >
                  <span style={{ display: 'flex', justifyContent: 'space-between', gap: '12px', color: '#F4F3EF', fontSize: '12px' }}>
                    <span>{wallet.label}</span>
                    <span style={{ color: '#8F8D8E' }}>{balances[wallet.id] ?? '…'}</span>
                  </span>
                  <span style={{ display: 'block', marginTop: '6px', color: '#8F8D8E', fontSize: '11px', wordBreak: 'break-all' }}>
                    {wallet.address}
                  </span>
                </button>
              );
            })}
          </div>
        </div>
      )}
    </div>
  );
}

function HostedWalletConnect() {
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
        className={walletButtonClass}
      >
        {connecting ? 'Connecting...' : 'Connect Wallet'}
      </button>
    );
  }

  // 3. Connected UI
  const short = `${address.slice(0, 6)}…${address.slice(-4)}`;
  return (
    <div style={{ display: 'flex', alignItems: 'center', gap: '8px' }}>
      <div className={`${walletButtonClass} flex items-center justify-center`}>
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
