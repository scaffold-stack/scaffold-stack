import { atom } from 'jotai';
import { atomWithStorage } from 'jotai/utils';

// Automatically persists to localStorage under the key 'stx-address'
export const addressAtom = atomWithStorage<string | null>('stx-address', null);

// A simple boolean to track if the client has hydrated
export const isMountedAtom = atom(false);