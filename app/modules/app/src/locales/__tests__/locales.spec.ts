import {describe, expect, it} from 'vitest';
import deAccounts from '../de/accounts.json';
import deGlobal from '../de/global.json';
import deRollback from '../de/rollback.json';
import deSettings from '../de/settings.json';
import deShell from '../de/shell.json';
import deStatus from '../de/status.json';
import deUpdate from '../de/update.json';
import enAccounts from '../en/accounts.json';
import enGlobal from '../en/global.json';
import enRollback from '../en/rollback.json';
import enSettings from '../en/settings.json';
import enShell from '../en/shell.json';
import enStatus from '../en/status.json';
import enUpdate from '../en/update.json';

const locales = {
  de: {
    accounts: deAccounts,
    global: deGlobal,
    rollback: deRollback,
    settings: deSettings,
    shell: deShell,
    status: deStatus,
    update: deUpdate,
  },
  en: {
    accounts: enAccounts,
    global: enGlobal,
    rollback: enRollback,
    settings: enSettings,
    shell: enShell,
    status: enStatus,
    update: enUpdate,
  },
};

/** Return sorted interpolation placeholders contained in one translation. */
function placeholders(translation: string): string[] {
  return [...translation.matchAll(/{{([^}]+)}}/g)].map(match => match[1]!).sort();
}

describe('locales', () => {
  it('keeps German namespace and translation keys aligned with the English source locale', () => {
    expect(Object.keys(locales.de).sort()).toEqual(Object.keys(locales.en).sort());

    for (const namespace of Object.keys(locales.en) as Array<keyof typeof locales.en>) {
      expect(Object.keys(locales.de[namespace]).sort(), namespace)
        .toEqual(Object.keys(locales.en[namespace]).sort());
    }
  });

  it('preserves interpolation placeholders in every German translation', () => {
    for (const namespace of Object.keys(locales.en) as Array<keyof typeof locales.en>) {
      const english = locales.en[namespace] as Record<string, string>;
      const german = locales.de[namespace] as Record<string, string>;
      for (const key of Object.keys(english)) {
        expect(placeholders(german[key]!), `${namespace}.${key}`)
          .toEqual(placeholders(english[key]!));
      }
    }
  });

  it('contains only non-empty translation strings', () => {
    for (const [locale, namespaces] of Object.entries(locales)) {
      for (const [namespace, translations] of Object.entries(namespaces)) {
        for (const [key, translation] of Object.entries(translations)) {
          expect(translation, `${locale}.${namespace}.${key}`).not.toBe('');
        }
      }
    }
  });
});
