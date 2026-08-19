import i18next from 'i18next';
import {beforeEach, describe, expect, it, vi} from 'vitest';
import accountsDefaults from '../locales/en/accounts.json';
import globalDefaults from '../locales/en/global.json';

const mocks = vi.hoisted(() => ({
  getLanguage: vi.fn(),
  log: {error: vi.fn(), warn: vi.fn()},
  setLanguage: vi.fn(),
}));

vi.mock('@app/commands/language', () => ({
  getAppLanguage: mocks.getLanguage,
  setAppLanguage: mocks.setLanguage,
}));
vi.mock('@app/log', () => ({getLogger: () => mocks.log}));

/** Import a fresh translation singleton for one test. */
async function importI18n() {
  vi.resetModules();
  return import('../i18n');
}

describe('i18n', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    document.documentElement.lang = '';
    mocks.getLanguage.mockResolvedValue('de');
    mocks.setLanguage.mockResolvedValue(undefined);
  });

  it('uses local English defaults synchronously before initialization', async () => {
    const {useI18n} = await importI18n();
    const {t} = useI18n('accounts', accountsDefaults);

    expect(t('server', {server: 106})).toBe('Server 106');
    expect(useI18n('accounts', {...accountsDefaults, cancel: 'Fallback cancel'}).t('cancel'))
      .toBe('Cancel');
    expect(useI18n('custom', {custom: 'Custom default'}).t('custom')).toBe('Custom default');
  });

  it('rejects language changes before initialization', async () => {
    const {useI18n} = await importI18n();

    await expect(
      useI18n('global', globalDefaults).setLanguage('de'),
    ).rejects.toThrow('Translations have not been initialized.');
  });

  it('initializes the backend-selected language once', async () => {
    const {initI18n, useI18n} = await importI18n();

    await initI18n();
    await initI18n();

    expect(mocks.getLanguage).toHaveBeenCalledOnce();
    expect(mocks.getLanguage).toHaveBeenCalledWith(navigator.language);
    expect(useI18n('global', globalDefaults).language.value).toBe('de');
    expect(useI18n('global', globalDefaults).t('cancel')).toBe('Abbrechen');
    expect(document.documentElement.lang).toBe('de');
  });

  it('falls back to English when the backend language cannot be resolved', async () => {
    const reason = new Error('offline');
    mocks.getLanguage.mockRejectedValue(reason);
    const {initI18n, useI18n} = await importI18n();

    await initI18n();

    expect(useI18n('global', globalDefaults).language.value).toBe('en');
    expect(mocks.log.warn).toHaveBeenCalledWith(
      'Failed to resolve application language; using English:',
      reason,
    );
  });

  it('changes and persists the language immediately', async () => {
    const {initI18n, useI18n} = await importI18n();
    await initI18n();

    await useI18n('global', globalDefaults).setLanguage('en');

    expect(useI18n('global', globalDefaults).language.value).toBe('en');
    expect(document.documentElement.lang).toBe('en');
    expect(mocks.setLanguage).toHaveBeenCalledWith('en');
  });

  it('keeps the selected language when persistence fails and logs the failure', async () => {
    const reason = new Error('disk full');
    mocks.setLanguage.mockRejectedValue(reason);
    const {initI18n, useI18n} = await importI18n();
    await initI18n();

    await useI18n('global', globalDefaults).setLanguage('en');

    expect(useI18n('global', globalDefaults).language.value).toBe('en');
    expect(mocks.log.error).toHaveBeenCalledWith('Failed to persist application language:', reason);
  });

  it('parses valid locale paths and ignores unrelated modules', async () => {
    const {parseLocaleModules} = await importI18n();

    expect(parseLocaleModules({
      './locales/en/global.json': {close: 'Close'},
      './unrelated.json': {ignored: 'Ignored'},
    })).toEqual({
      namespaces: ['global'],
      resources: {en: {global: {close: 'Close'}}},
    });
  });

  it('rejects non-string locale values', async () => {
    const {parseLocaleModules} = await importI18n();

    expect(() => parseLocaleModules({'./locales/en/global.json': {close: 42}}))
      .toThrow('Locale module ./locales/en/global.json contains a non-string value at key "close".');
  });

  it('resolves direct global and namespace references without expanding chains recursively', async () => {
    const {prepareLocaleResources} = await importI18n();

    expect(prepareLocaleResources({
      en: {
        global: {appName: 'Daystrom', title: '{{appName}}', chained: '{{title}}'},
        shell: {name: 'Shell', own: '{{name}}', heading: '{{appName}} {{own}}'},
      },
    })).toEqual({
      en: {
        global: {appName: 'Daystrom', title: 'Daystrom', chained: '{{appName}}'},
        shell: {name: 'Shell', own: 'Shell', heading: 'Daystrom {{name}}'},
      },
    });
  });

  it('supports global fallbacks and namespace overrides', async () => {
    const {prepareLocaleResources} = await importI18n();
    const prepared = prepareLocaleResources({
      en: {global: {close: 'Global close'}, shell: {close: 'Shell close'}},
    });
    const translations = i18next.createInstance();
    await translations.init({
      defaultNS: 'shell',
      fallbackNS: 'global',
      lng: 'en',
      resources: prepared,
    });

    expect(translations.t('close')).toBe('Shell close');

    const fallbackTranslations = i18next.createInstance();
    await fallbackTranslations.init({
      defaultNS: 'shell',
      fallbackNS: 'global',
      lng: 'en',
      resources: {en: {global: {close: 'Global close'}, shell: {}}},
    });
    expect(fallbackTranslations.t('close')).toBe('Global close');
  });

  it('prepares a global namespace for locales that omit it', async () => {
    const {prepareLocaleResources} = await importI18n();

    expect(prepareLocaleResources({en: {shell: {title: 'Daystrom'}}}))
      .toEqual({en: {global: {}, shell: {title: 'Daystrom'}}});
  });
});
