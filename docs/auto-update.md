# Daystrom auto-update and rollback

Status: Implemented; production release validation pending

This document is the technical contract for releasing, updating, and rolling back Project Daystrom.

## Boundaries

- Auto-update changes Daystrom and its bundled mod, never STFC itself.
- Installation and rollback always require an explicit user action.
- Daystrom never terminates a running game to update or restore itself.
- The Rust backend owns discovery, trust decisions, downloads, installation, and rollback. The frontend only displays
  domain state and invokes narrowly scoped commands.
- A rollback repairs a regression in Daystrom or its bundled mod. It is not a recovery mechanism for an incompatible
  Scopely game update.

## Release flow

1. Run the manually dispatched `Prepare Release` workflow with the intended source revision and version.
2. The workflow verifies the version and tag, then builds Windows x64 and universal macOS artefacts.
3. Platform installers, Tauri updater packages, rollback metadata, detached signatures, checksums, and `latest.json` are
   attached to a draft release.
4. Test both installers from the draft. Drafts remain invisible to the production update endpoint.
5. Publish the tested draft manually in the GitHub UI.
6. `Finalize Published Release` adds the rollout anchor to `latest.json` and refreshes only its entry in `SHA256SUMS`.

Release preparation may replace assets on the same draft only while its source revision and version remain unchanged.
After publication, installers, updater packages, rollback metadata, and all detached signatures are immutable.
Finalization modifies only `latest.json` and `SHA256SUMS`; it never re-signs release content.

`0.9.0` is the first updater-enabled release. Its workflow contains a bootstrap exception because no earlier release has
a `latest.json`. Remove that exception when preparing `0.10.0`.

## Trust model

Platform trust and updater trust are separate:

- Windows Authenticode signs the NSIS installer.
- Apple code signing and notarization establish trust in the macOS application.
- The Tauri updater key signs forward-update packages and rollback metadata.

Windows signing changes the installer bytes after Tauri bundling, so release automation regenerates the detached Tauri
signature after Authenticode signing.

The updater private key and password exist only in GitHub Actions secrets as `TAURI_SIGNING_PRIVATE_KEY` and
`TAURI_SIGNING_PRIVATE_KEY_PASSWORD`. The encrypted private key and its password must also be backed up separately. Key
rotation requires a migration release trusted by the previous key.

The manifest itself is delivered through GitHub over HTTPS but is not signed. Daystrom therefore installs a package
only when:

- its detached Tauri signature validates against the embedded public key;
- its version and platform match the reviewed update and signed metadata;
- its credential-free HTTPS URL belongs to the configured `MBurchard/project-daystrom` release source;
- a rollback targets exactly the predecessor authorized by the target release's signed metadata.

Rollback metadata is signed in its compact UTF-8 form and embedded in `latest.json` as that exact string. The client
verifies the original bytes before strict parsing; unknown fields are rejected.

## Discovery and rollout

Production uses `https://github.com/MBurchard/project-daystrom/releases/latest/download/latest.json`. Daystrom checks:

- after normal startup;
- every six hours while running;
- when the user selects `Check for Daystrom updates`.

Updates are delayed according to the difference between the installed version and the target version:

- patch update within the installed major and minor line: no delay;
- minor update within the installed major line: at least 12 hours;
- major update: at least 24 hours.

`pub_date` is the rollout anchor of a major/minor release line. A patch inherits the anchor of the first published
release in its line. Consequently, `0.10.1` can replace `0.10.0` without restarting the 12-hour timer for users on
`0.9.x`; users already on `0.10.x` receive it immediately. A new minor or major line receives its anchor during
publication finalization.

The six-hour polling interval is unchanged, so automatic discovery normally exposes minor updates after 12–18 hours
and major updates after 24–30 hours. Manual checks honour the same minimum delays.

An update found at startup appears only in the main window. A new version first discovered by a later periodic check may
also produce a native notification; clicking it brings Daystrom forward. `Later` dismisses the version for the current
process. Restarting Daystrom or checking manually shows it again.

Debug builds may use `DAYSTROM_UPDATE_ENDPOINT` and `DAYSTROM_UPDATE_INTERVAL_SECONDS`. An explicit debug endpoint
permits installation and bypasses production rollout delays. Release builds ignore both variables.

## Forward update

Selecting `Install update` causes the backend to:

1. re-fetch the manifest and require the version to match the update reviewed by the user;
2. verify or download the target release's authorized rollback predecessor;
3. download and verify the selected update package;
4. persist settings and the pending installation state;
5. flush logging and complete coordinated shutdown;
6. invoke Tauri's native platform installer.

Rollback retention and update download have separate visible phases and progress. Installation cannot begin until both
packages and their signatures are verified.

A running game remains open with the mod already loaded in its process. After Daystrom restarts, that mod reconnects
through WebSocket discovery. The newly bundled mod becomes active only after a later game restart and, on Windows,
successful deployment.

## Rollback cache

Daystrom retains exactly one rollback release: the direct published predecessor authorized by the target release. This
is independent of the version currently installed. For example, updating directly from `0.9.0` to `0.11.0` retains
`0.10.0`, not `0.9.0`.

The cache stores verified original updater packages in the application-data directory; it never copies the live
installation directory. Before installation, it must contain:

- the verified target package;
- the verified package of the target's authorized predecessor;
- valid signed predecessor metadata;
- a one-generation backup of persisted settings.

After a successful update, the downloaded target package becomes the current-package cache. A subsequent update reuses
it when it matches the new target's signed predecessor metadata; otherwise Daystrom downloads the required predecessor.
The old rollback package is deleted only after the replacement is safely stored.

The first update from manually installed `0.9.0` to `0.10.0` may download both versions because no current-package
cache exists yet. Consecutive steady-state updates normally download only the new package.

## One-click rollback

Diagnostics expose `Restore previous Daystrom version <version>` when a valid rollback is available. The backend then:

1. re-verifies the package, platform, version, signature, and predecessor authorization;
2. verifies and restores the one-generation settings backup while retaining successor settings for failure recovery;
3. records durably that the restored bundled mod still needs activation;
4. performs coordinated shutdown and invokes the native platform installer;
5. keeps the pending mod restore visible until no running game blocks activation.

The frontend cannot provide a URL, package path, installation path, or arbitrary version. Profiles are never replaced.
If the installer fails, successor settings are restored and the rollback remains available.

Rolling back does not modify a mod already loaded into STFC. After the game closes, Windows deploys the restored DLL for
the next start; on macOS the next game launch through Daystrom injects the restored library. The durable pending marker
survives Daystrom restarts until activation succeeds.

After rollback, automatic checks suppress the rejected successor. A manual update check is the explicit user action
that makes it available again.

## Compatibility and recovery

Every release must read settings and profiles written by its immediate successor. Persisted-data changes must therefore
be additive, reversible, preserved for one release, or recoverable from the settings backup. Profiles are user data and
must never be silently discarded.

Downloads and cache updates are interruption-safe: temporary packages become trusted state only after complete download
and signature verification. Failed updates must preserve the existing rollback package and settings backup.

One-click rollback assumes the updated Daystrom application can start. If it cannot, manually reinstall the previous
published installer. A separately launchable signed recovery tool remains a possible future improvement, not part of
the current contract.

## Release verification

The release revision must have passed lint, typecheck, backend, frontend, mod, and cross-platform game-compatibility
checks. Before publishing a draft, verify:

- Windows installation and Authenticode signature;
- macOS installation, code signature, and notarization;
- Tauri signatures for both updater packages and rollback metadata;
- draft isolation from the production endpoint;
- update discovery after publication and rollout finalization;
- installation and WebSocket reconnection without terminating a running game;
- one-click rollback on Windows and macOS;
- compatibility with settings and profiles written by the successor;
- safe failure for interrupted, malformed, denied, or tampered updates;
- retention of only one predecessor after consecutive updates.

The first production auto-update validation requires a controlled update from `0.9.0` to `0.10.0` after publication.
