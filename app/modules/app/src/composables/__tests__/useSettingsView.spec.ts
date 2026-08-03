import {
  MAX_CONFIGURED_SLIDER_LIMIT,
  normalizeSliderLimit,
  STANDARD_RECRUIT_MAX,
} from '@app/composables/useSettingsView';

import {describe, expect, it} from 'vitest';

describe('normalizeSliderLimit', () => {
  it('caps Standard Recruit at its supported maximum', () => {
    expect(normalizeSliderLimit('500', STANDARD_RECRUIT_MAX)).toBe(150);
  });

  it('caps alliance donations at the largest supported setting value', () => {
    expect(normalizeSliderLimit('4294967296', MAX_CONFIGURED_SLIDER_LIMIT))
      .toBe(MAX_CONFIGURED_SLIDER_LIMIT);
  });

  it('truncates fractional values', () => {
    expect(normalizeSliderLimit('87.9', STANDARD_RECRUIT_MAX)).toBe(87);
  });

  it.each(['50', '20', 'not-a-number'])('maps %s to the unchanged game default', (value) => {
    expect(normalizeSliderLimit(value, STANDARD_RECRUIT_MAX)).toBeNull();
  });
});
