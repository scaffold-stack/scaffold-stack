import { describe, expect, it } from 'vitest';
import { devnetInclusionStatus, formatStx, getDevnetBurners, parseMicrostacks } from './devnet';

describe('devnet wallets', () => {
  it('derives the public Clarinet deployer and three funded wallets', () => {
    const burners = getDevnetBurners();
    expect(burners.map(burner => burner.id)).toEqual([
      'deployer',
      'wallet_1',
      'wallet_2',
      'wallet_3',
    ]);
    expect(burners[0]?.address).toBe('ST1PQHQKV0RJXZFY1DGX8MNSNYVE3VGZJSRTPGZGM');
    expect(burners[1]?.address).toBe('ST1SJ3DTE5DN7X54YDH5D64R3BCB6A2AG2ZQ8YPD5');
  });

  it('treats a tx as included once the account nonce moves past it', () => {
    expect(devnetInclusionStatus(3n, 2n)).toEqual({ state: 'success' });
    expect(devnetInclusionStatus(2n, 2n)).toEqual({ state: 'pending' });
  });

  it('formats core-node balances', () => {
    expect(parseMicrostacks('0x5af3107a4000')).toBe(100_000_000_000_000n);
    expect(formatStx(100_000_000_000_000n)).toBe('100000000 STX');
    expect(formatStx(1_500_000n)).toBe('1.5 STX');
  });
});
