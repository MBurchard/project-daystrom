import type {AppLanguage} from '@generated/AppLanguage';
import type {DaystromReleaseNotes} from '@generated/DaystromReleaseNotes';

/**
 * Select release notes for the active interface language, falling back to English.
 * @param notes - Bilingual notes supplied by the backend.
 * @param language - Active application language.
 * @returns Localized plain-text notes, or null when the manifest provides none.
 */
export function releaseNotesForLanguage(
  notes: DaystromReleaseNotes | null,
  language: AppLanguage,
): string | null {
  if (!notes) {
    return null;
  }
  return language === 'de' ? notes.de : notes.en;
}
