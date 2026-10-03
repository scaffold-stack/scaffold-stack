import { cvToJSON, type ClarityValue } from '@stacks/transactions';

/**
 * Turn a Clarity value into plain JavaScript.
 *
 * Responses keep their ok/err flag: `{ ok: true, value }` or `{ ok: false, value }`.
 * Nested tuples, lists, and optionals are unwrapped too, so a UI does not have to
 * walk `{ type, value }` objects.
 */
export function readClarity(cv: ClarityValue): unknown {
  return decodeClarityJson(cvToJSON(cv));
}

function decodeClarityJson(node: unknown): unknown {
  if (node == null || typeof node !== 'object') return node;
  const rec = node as { type?: unknown; value?: unknown; success?: unknown };
  const type = typeof rec.type === 'string' ? rec.type : '';

  if (type === 'uint' || type === 'int') return BigInt(String(rec.value));
  if (type === 'bool') return rec.value === true || rec.value === 'true';
  if (type === 'principal' || type === 'trait_reference') return rec.value;
  if (type === 'none') return null;
  if (
    type === 'string-ascii' ||
    type === 'string-utf8' ||
    type.startsWith('(string-ascii') ||
    type.startsWith('(string-utf8')
  ) {
    return rec.value;
  }
  if (type === 'buff' || type === 'buffer' || type.startsWith('(buff')) return rec.value;

  if (type === 'ok') return { ok: true, value: decodeClarityJson(rec.value) };
  if (type === 'err') return { ok: false, value: decodeClarityJson(rec.value) };
  if (type.startsWith('(response')) {
    return { ok: rec.success === true, value: decodeClarityJson(rec.value) };
  }

  if (type.startsWith('(optional') || type === 'some') {
    if (rec.value == null) return null;
    return decodeClarityJson(rec.value);
  }

  if (type.startsWith('(tuple') || type === 'tuple') {
    const value = rec.value;
    if (!value || typeof value !== 'object' || Array.isArray(value)) return {};
    const out: Record<string, unknown> = {};
    for (const [key, child] of Object.entries(value as Record<string, unknown>)) {
      out[key] = decodeClarityJson(child);
    }
    return out;
  }

  if (type.startsWith('(list') || type === 'list') {
    return Array.isArray(rec.value) ? rec.value.map((child) => decodeClarityJson(child)) : [];
  }

  return rec.value ?? node;
}
