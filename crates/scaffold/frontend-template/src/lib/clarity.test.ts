import { Cl } from '@stacks/transactions';
import { describe, expect, it } from 'vitest';
import { readClarity } from './clarity';

describe('readClarity', () => {
  it('keeps ok and unwraps nested tuple fields', () => {
    const cv = Cl.ok(
      Cl.tuple({
        amount: Cl.uint(12),
        note: Cl.some(Cl.stringUtf8('hi')),
        missing: Cl.none(),
      }),
    );
    expect(readClarity(cv)).toEqual({
      ok: true,
      value: { amount: 12n, note: 'hi', missing: null },
    });
  });

  it('keeps err instead of returning a bare uint', () => {
    expect(readClarity(Cl.error(Cl.uint(100)))).toEqual({ ok: false, value: 100n });
  });

  it('unwraps a list of principals inside an ok', () => {
    const cv = Cl.ok(Cl.list([Cl.principal('ST1PQHQKV0RJXZFY1DGX8MNSNYVE3VGZJSRTPGZGM')]));
    expect(readClarity(cv)).toEqual({
      ok: true,
      value: ['ST1PQHQKV0RJXZFY1DGX8MNSNYVE3VGZJSRTPGZGM'],
    });
  });

  it('unwraps a nested summary: int, bool, buff, optional, and list of tuples', () => {
    const cv = Cl.ok(
      Cl.tuple({
        count: Cl.uint(2),
        surplus: Cl.int(-4),
        open: Cl.bool(true),
        tag: Cl.bufferFromHex('abcd'),
        note: Cl.some(Cl.stringAscii('desk')),
        missing: Cl.none(),
        jobs: Cl.list([
          Cl.tuple({
            id: Cl.uint(7),
            memo: Cl.some(Cl.stringUtf8('retainer')),
          }),
        ]),
      }),
    );
    expect(readClarity(cv)).toEqual({
      ok: true,
      value: {
        count: 2n,
        surplus: -4n,
        open: true,
        tag: '0xabcd',
        note: 'desk',
        missing: null,
        jobs: [{ id: 7n, memo: 'retainer' }],
      },
    });
  });

  it('unwraps an err tuple and an ok none', () => {
    expect(
      readClarity(Cl.error(Cl.tuple({ code: Cl.uint(1), reason: Cl.stringAscii('full') }))),
    ).toEqual({ ok: false, value: { code: 1n, reason: 'full' } });
    expect(readClarity(Cl.ok(Cl.none()))).toEqual({ ok: true, value: null });
    expect(readClarity(Cl.list([]))).toEqual([]);
  });
});
