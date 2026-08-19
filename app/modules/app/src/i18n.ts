import type {AppLanguage} from '@generated/AppLanguage';
import type {i18n as I18nInstance, TOptions} from 'i18next';
import {getAppLanguage, setAppLanguage} from '@app/commands/language';
import {getLogger} from '@app/log';
import i18next from 'i18next';
import {readonly, ref} from 'vue';

/** Translation options passed through to i18next. */
export type TranslationOptions = TOptions;

/** Locale module content returned by Vite's eager JSON glob import. */
export type LocaleModuleContent = Record<string, unknown>;

/** Glob-imported locale modules indexed by their build-time paths. */
export type LocaleModules = Record<string, LocaleModuleContent>;

/** Flat locale resources indexed by language and namespace. */
export type LocaleResources = Record<string, Record<string, Record<string, string>>>;

/** Parsed locale modules and their stable namespace list. */
export interface ParsedLocaleModules {
  /** Available translation namespaces. */
  readonly namespaces: string[];
  /** Validated translations indexed by language and namespace. */
  readonly resources: LocaleResources;
}

const localeModules = import.meta.glob<Record<string, string>>(
  './locales/**/*.json',
  {eager: true, import: 'default'},
);

const GLOBAL_NAMESPACE = 'global';
const LOCALE_PATH_RE = /\/([^/]+)\/([^/]+)\.json$/;
const PLACEHOLDER_RE = /\{\{(\w+)}}/g;
const log = getLogger('I18n');
const language = ref<AppLanguage>('en');
let initialized = false;
let engine: I18nInstance | undefined;

/** Validate one flat JSON locale module. */
function validateLocaleModule(path: string, content: LocaleModuleContent): Record<string, string> {
  const validated: Record<string, string> = {};
  for (const [key, value] of Object.entries(content)) {
    if (typeof value !== 'string') {
      throw new TypeError(`Locale module ${path} contains a non-string value at key "${key}".`);
    }
    validated[key] = value;
  }
  return validated;
}

/** Parse Vite locale modules into i18next resources. */
export function parseLocaleModules(modules: LocaleModules): ParsedLocaleModules {
  const parsedResources: LocaleResources = {};
  const namespaces = new Set<string>();

  for (const [path, content] of Object.entries(modules)) {
    const match = path.match(LOCALE_PATH_RE);
    if (!match) {
      continue;
    }
    const [, locale, namespace] = match;
    parsedResources[locale!] ??= {};
    parsedResources[locale!]![namespace!] = validateLocaleModule(path, content);
    namespaces.add(namespace!);
  }

  return {namespaces: [...namespaces], resources: parsedResources};
}

/** Resolve direct references against one namespace without expanding chains recursively. */
function resolveNamespaceReferences(
  values: Record<string, string>,
  references: Record<string, string>,
): Record<string, string> {
  const resolved: Record<string, string> = {};
  for (const [key, value] of Object.entries(values)) {
    resolved[key] = value.replaceAll(
      PLACEHOLDER_RE,
      (match, reference: string) => reference === key ? match : references[reference] ?? match,
    );
  }
  return resolved;
}

/** Prepare global fallbacks and module overrides for every bundled language. */
export function prepareLocaleResources(localeResources: LocaleResources): LocaleResources {
  const prepared: LocaleResources = {};

  for (const [locale, localeNamespaces] of Object.entries(localeResources)) {
    const rawGlobal = localeNamespaces[GLOBAL_NAMESPACE] ?? {};
    const global = resolveNamespaceReferences(rawGlobal, rawGlobal);
    prepared[locale] = {[GLOBAL_NAMESPACE]: global};

    for (const [namespace, values] of Object.entries(localeNamespaces)) {
      if (namespace === GLOBAL_NAMESPACE) {
        continue;
      }
      const selfResolved = resolveNamespaceReferences(values, values);
      prepared[locale]![namespace] = resolveNamespaceReferences(selfResolved, global);
    }
  }

  return prepared;
}

const parsedLocaleModules = parseLocaleModules(localeModules);
const namespaces = parsedLocaleModules.namespaces;
const resources = prepareLocaleResources(parsedLocaleModules.resources);

/** Return global values used as default interpolation variables for one language. */
function globalVariables(nextLanguage: AppLanguage): Record<string, string> {
  return resources[nextLanguage]![GLOBAL_NAMESPACE]!;
}

/** Apply one language to the translation engine and document metadata. */
async function applyLanguage(nextLanguage: AppLanguage): Promise<void> {
  if (!engine) {
    throw new Error('Translations have not been initialized.');
  }
  engine.options.interpolation ??= {};
  engine.options.interpolation.defaultVariables = globalVariables(nextLanguage);
  await engine.changeLanguage(nextLanguage);
  language.value = nextLanguage;
  document.documentElement.lang = nextLanguage;
}

/** Initialize bundled translations and resolve the initial language before Vue mounts. */
export async function initI18n(): Promise<void> {
  if (initialized) {
    return;
  }

  let initialLanguage: AppLanguage = 'en';
  try {
    initialLanguage = await getAppLanguage(navigator.language);
  } catch (reason) {
    log.warn('Failed to resolve application language; using English:', reason);
  }

  engine = i18next.createInstance();
  await engine.init({
    defaultNS: GLOBAL_NAMESPACE,
    fallbackLng: 'en',
    fallbackNS: GLOBAL_NAMESPACE,
    interpolation: {
      defaultVariables: globalVariables(initialLanguage),
      escapeValue: false,
    },
    lng: initialLanguage,
    ns: namespaces,
    resources,
    returnNull: false,
  });
  language.value = initialLanguage;
  document.documentElement.lang = initialLanguage;
  initialized = true;
}

/** Resolve fallback text before the translation engine has initialized. */
function resolveFallbackText(
  namespace: string,
  key: string,
  defaultText: string,
  options?: TranslationOptions,
): string {
  const global = resources.en![GLOBAL_NAMESPACE]!;
  const text = resources.en![namespace]?.[key] ?? global[key] ?? defaultText;
  const values = {...global, ...(options as Record<string, unknown> | undefined)};
  return Object.entries(values).reduce(
    (resolved, [key, value]) => resolved.replaceAll(`{{${key}}}`, String(value)),
    text,
  );
}

/**
 * Expose typed translations from one namespace with local English defaults.
 *
 * @param namespace - Translation namespace to resolve before the global fallback.
 * @param defaults - English defaults defining the supported keys.
 * @returns Reactive language state, persistence, and a typed translation function.
 */
export function useI18n<const T extends Record<string, string>>(namespace: string, defaults: T) {
  return {
    language: readonly(language),
    setLanguage: changeLanguage,
    t<K extends Extract<keyof T, string>>(key: K, options?: TranslationOptions): string {
      if (!engine) {
        return resolveFallbackText(namespace, key, defaults[key], options);
      }
      return engine.t(key, defaults[key], {lng: language.value, ns: namespace, ...options});
    },
  };
}

/** Change the interface language immediately and persist the explicit selection. */
async function changeLanguage(nextLanguage: AppLanguage): Promise<void> {
  await applyLanguage(nextLanguage);
  try {
    await setAppLanguage(nextLanguage);
  } catch (reason) {
    log.error('Failed to persist application language:', reason);
  }
}
