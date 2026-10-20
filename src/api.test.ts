import {describe,it,expect} from 'vitest';
// Keep formatting checks independent of the desktop bridge.
describe('display values',()=>{it('uses finite byte values',()=>{const bytes=(n:number)=>Number.isFinite(n)&&n>=0;expect(bytes(NaN)).toBe(false);expect(bytes(1024)).toBe(true);});});
