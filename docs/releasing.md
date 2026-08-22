# Releasing Project Daystrom

## Prepare the release

1. Set the release version in `package.json` and `app/modules/backend/Cargo.toml`.
2. Add `release-notes/<version>.json` with reviewed German and English titles and matching lists of changes.
3. Run the `Prepare Release Draft` workflow for the intended revision and enter the version from `package.json`.

The workflow validates the versions and release notes, creates or updates the GitHub draft, builds the Windows and
macOS artefacts, signs the update packages and rollback metadata, and attaches `latest.json` and `SHA256SUMS`.

Signing credentials must be configured as described in [release-signing.md](release-signing.md).

## Publish the release

Install and test both platform builds from the draft, then publish it in the GitHub UI. Publishing triggers
`Finalize Published Release`, which adds the rollout timestamp to `latest.json` and updates its checksum.

A draft may be rebuilt only from the same version and source revision. Do not replace installers, update packages,
rollback metadata, signatures, or checksums after publication.

## Release-note compatibility

- `notes` in `latest.json` is always English because published 0.9.x clients read this field.
- `localized_notes` contains German and English for Daystrom 0.10.0 and newer. Other interface languages use English.
- Extend `localized_notes` only with additional fields or languages and keep `schema` at `1`.
- Keep `de` and `en` as plain-text strings with their current meaning.
- Use a new top-level manifest field for an incompatible format and retain `localized_notes` for published clients.

The release workflow generates the GitHub text and both manifest fields from the checked-in release-notes file.

## Rollout timing

- Patch updates have no minimum delay.
- Minor updates are delayed by at least 12 hours.
- Major updates are delayed by at least 24 hours.

The first published release in a minor or major line sets its rollout timestamp. Later patches in the same line inherit
that timestamp. Automatic checks run every six hours, and manual checks honour the same minimum delays.
