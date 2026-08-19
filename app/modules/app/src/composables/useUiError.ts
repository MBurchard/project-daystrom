import type {UiErrorCode} from '@generated/UiErrorCode';
import {useI18n} from '@app/i18n';
import errorDefaults from '@app/locales/en/errors.json';
import {getLogger} from '@app/log';

/** Require translations to contain exactly the generated backend error-code union. */
type ExactUiErrorTranslations<Translations extends Record<string, string>> =
  Exclude<UiErrorCode, keyof Translations> extends never ?
    Exclude<keyof Translations, UiErrorCode> extends never ?
      Translations :
      never :
    never;

const log = getLogger('UIError');
const errorTranslations: ExactUiErrorTranslations<typeof errorDefaults> = errorDefaults;
const errorCodes = new Set<string>(Object.keys(errorTranslations));

/** Translation helper for backend-owned, user-facing error codes. */
export interface UiErrorTranslations {
  /** Resolve a stable backend error code into the active interface language. */
  errorText: (error: UiErrorCode) => string;
}

/** Convert an arbitrary command rejection into a safe, displayable error code. */
export function normalizeUiError(reason: unknown): UiErrorCode {
  if (typeof reason === 'string' && errorCodes.has(reason)) {
    return reason as UiErrorCode;
  }
  log.error('Backend returned an unknown user-facing error:', reason);
  return 'unexpected';
}

/** Expose translated text for backend-owned, user-facing errors. */
export function useUiError(): UiErrorTranslations {
  const {t} = useI18n('errors', errorTranslations);
  return {errorText: error => t(error)};
}
