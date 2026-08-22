import {describe, expect, it} from 'vitest';
import bannerCategories from '../../components/toast-banner-categories.json';
import deAccounts from '../de/accounts.json';
import deErrors from '../de/errors.json';
import deGlobal from '../de/global.json';
import deRollback from '../de/rollback.json';
import deSafety from '../de/safety.json';
import deSettings from '../de/settings.json';
import deShell from '../de/shell.json';
import deStatus from '../de/status.json';
import deToast from '../de/toast.json';
import deUpdate from '../de/update.json';
import enAccounts from '../en/accounts.json';
import enErrors from '../en/errors.json';
import enGlobal from '../en/global.json';
import enRollback from '../en/rollback.json';
import enSafety from '../en/safety.json';
import enSettings from '../en/settings.json';
import enShell from '../en/shell.json';
import enStatus from '../en/status.json';
import enToast from '../en/toast.json';
import enUpdate from '../en/update.json';
import tlhAccounts from '../tlh/accounts.json';
import tlhErrors from '../tlh/errors.json';
import tlhGlobal from '../tlh/global.json';
import tlhRollback from '../tlh/rollback.json';
import tlhSafety from '../tlh/safety.json';
import tlhSettings from '../tlh/settings.json';
import tlhShell from '../tlh/shell.json';
import tlhStatus from '../tlh/status.json';
import tlhToast from '../tlh/toast.json';
import tlhUpdate from '../tlh/update.json';

const locales = {
  de: {
    accounts: deAccounts,
    errors: deErrors,
    global: deGlobal,
    rollback: deRollback,
    safety: deSafety,
    settings: deSettings,
    shell: deShell,
    status: deStatus,
    toast: deToast,
    update: deUpdate,
  },
  en: {
    accounts: enAccounts,
    errors: enErrors,
    global: enGlobal,
    rollback: enRollback,
    safety: enSafety,
    settings: enSettings,
    shell: enShell,
    status: enStatus,
    toast: enToast,
    update: enUpdate,
  },
  tlh: {
    accounts: tlhAccounts,
    errors: tlhErrors,
    global: tlhGlobal,
    rollback: tlhRollback,
    safety: tlhSafety,
    settings: tlhSettings,
    shell: tlhShell,
    status: tlhStatus,
    toast: tlhToast,
    update: tlhUpdate,
  },
};

/** Return sorted interpolation placeholders contained in one translation. */
function placeholders(translation: string): string[] {
  return [...translation.matchAll(/{{([^}]+)}}/g)].map(match => match[1]!).sort();
}

describe('locales', () => {
  it('keeps translated namespaces and keys aligned with the English source locale', () => {
    for (const [locale, translations] of Object.entries(locales)) {
      expect(Object.keys(translations).sort(), locale).toEqual(Object.keys(locales.en).sort());

      for (const namespace of Object.keys(locales.en) as Array<keyof typeof locales.en>) {
        expect(Object.keys(translations[namespace]).sort(), `${locale}.${namespace}`)
          .toEqual(Object.keys(locales.en[namespace]).sort());
      }
    }
  });

  it('preserves interpolation placeholders in every translation', () => {
    for (const [locale, translations] of Object.entries(locales)) {
      for (const namespace of Object.keys(locales.en) as Array<keyof typeof locales.en>) {
        const english = locales.en[namespace] as Record<string, string>;
        const translated = translations[namespace] as Record<string, string>;
        for (const key of Object.keys(english)) {
          expect(placeholders(translated[key]!), `${locale}.${namespace}.${key}`)
            .toEqual(placeholders(english[key]!));
        }
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

  it('keeps the mandatory Klingon safety notice in understandable English', () => {
    expect(tlhSafety).toEqual(enSafety);
  });

  it('translates every configured toast-banner type', () => {
    const configuredTypes = Object.values(bannerCategories).flat().sort();
    expect(Object.keys(enToast).sort()).toEqual(configuredTypes);
  });
});
