import type {GameSettings} from '@generated/GameSettings';

import {beforeEach, describe, expect, it, vi} from 'vitest';

// ---- Mocks --------------------------------------------------------------------------

const mockInvoke = vi.hoisted(() => vi.fn());

vi.mock('@tauri-apps/api/core', () => ({
  invoke: mockInvoke,
}));

// ---- Tests --------------------------------------------------------------------------

describe('settings commands', () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  describe('getGameSettings', () => {
    it('invokes the correct command without args', async () => {
      const expected: GameSettings = {ui: {scale: 120}};
      mockInvoke.mockResolvedValue(expected);

      const {getGameSettings} = await import('../settings');
      const result = await getGameSettings();

      expect(mockInvoke).toHaveBeenCalledWith('get_game_settings');
      expect(result).toEqual(expected);
    });
  });

  describe('setGameSettings', () => {
    it('invokes with the correct command and parameter key', async () => {
      mockInvoke.mockResolvedValue(undefined);

      const {setGameSettings} = await import('../settings');
      const settings: GameSettings = {ui: {scale: 75}};
      await setGameSettings(settings);

      // The key MUST be "settings" to match the Rust parameter name.
      // If this breaks, the backend will reject the call at runtime.
      expect(mockInvoke).toHaveBeenCalledWith('set_game_settings', {settings});
    });

    it('passes the full settings object, not a partial', async () => {
      mockInvoke.mockResolvedValue(undefined);

      const {setGameSettings} = await import('../settings');
      const settings: GameSettings = {ui: {scale: 100}};
      await setGameSettings(settings);

      const args = mockInvoke.mock.calls[0][1] as {settings: GameSettings};
      expect(args.settings).toHaveProperty('ui');
      expect(args.settings.ui).toHaveProperty('scale');
    });
  });
});
