import type {ProfileInfo} from '@generated/ProfileInfo';
import {resolveSelectedProfileStem} from '@app/components/accountTabs';
import {describe, expect, it} from 'vitest';

const PROFILES: ProfileInfo[] = [
  {name: 'Test Alpha', server: 1, stem: '1_TestAlpha', primary: true},
  {name: 'Test Beta', server: 2, stem: '2_TestBeta', primary: false},
];

describe('resolveSelectedProfileStem', () => {
  it('retains an account that still exists', () => {
    expect(resolveSelectedProfileStem(PROFILES, '2_TestBeta')).toBe('2_TestBeta');
  });

  it('selects the first account when the previous account disappeared', () => {
    expect(resolveSelectedProfileStem(PROFILES, '3_Removed')).toBe('1_TestAlpha');
  });

  it('clears the selection when no accounts exist', () => {
    expect(resolveSelectedProfileStem([], '1_TestAlpha')).toBeNull();
  });
});
