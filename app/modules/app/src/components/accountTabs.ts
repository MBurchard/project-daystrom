import type {ProfileInfo} from '@generated/ProfileInfo';

/**
 * Retain the selected profile when possible, or fall back to the first known account.
 *
 * @param profiles - Current profiles in backend-defined order.
 * @param selectedStem - Previously selected profile stem.
 * @returns The retained or fallback profile stem, or null when no profiles exist.
 */
export function resolveSelectedProfileStem(profiles: ProfileInfo[], selectedStem: string | null): string | null {
  return profiles.some(profile => profile.stem === selectedStem) ? selectedStem : profiles[0]?.stem ?? null;
}
