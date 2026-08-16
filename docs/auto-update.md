# Daystrom auto-update and rollback

Status: Approved; implementation in progress

This document defines the intended release, update, and rollback architecture for Project Daystrom. It is the source of truth for the feature until the implementation supersedes individual details.

## Goals

- Build Windows and macOS releases before they become visible to installed clients.
- Let a maintainer test both platform builds and publish the release manually.
- Offer signed Daystrom updates without silently forcing their installation.
- Keep a running game alive while Daystrom restarts for an update.
- Retain exactly one previous Daystrom release for a one-click rollback.
- Reject tampered, unsigned, and arbitrary downgrade packages.

## Scope

This feature updates Project Daystrom and its bundled mod. It does not update or downgrade STFC itself.

A rollback is appropriate when a Daystrom update or its bundled mod causes a regression. It is not a recovery mechanism for an incompatible Scopely game update. The UI must make this distinction explicit.

## Release preparation

GitHub draft releases form the release candidate boundary. Saving a draft release is not used as the workflow trigger because GitHub does not reliably emit release events for drafts.

The repository will provide a manually dispatched `Prepare Release` workflow:

1. The maintainer selects the exact source revision and supplies the release version.
2. The workflow verifies that the requested version matches `package.json` and that the release and tag do not conflict with an existing published release.
3. It creates or updates a draft GitHub release for that immutable revision.
4. The existing Windows x64 and universal macOS builds run in parallel.
5. Both builds produce their normal installers and Tauri updater artifacts.
6. The workflow signs the updater artifacts, verifies the signatures, and generates `latest.json` only after both platform builds succeed.
7. Installers, updater packages, detached signatures, checksums, and `latest.json` are attached to the draft.
8. The workflow stops without publishing the release.

The maintainer downloads and tests both installers from the draft. Publication remains a deliberate manual action in the GitHub UI. Publishing must not start another build.

Draft releases are not served through GitHub's normal `releases/latest` route. Existing installations therefore continue to see the previous stable release until the draft is published.

Re-running release preparation may repair or replace assets on the same draft only when the source revision and version are unchanged. A published release is immutable from the workflow's perspective.

## Signing and trust

Tauri updater signatures are required in addition to platform signing:

- Windows Authenticode signs the installer for Windows.
- Apple code signing and notarization establish trust in the macOS application.
- The Tauri updater key signs the cross-version update payload consumed by Daystrom.

The Tauri private key and its password live only in GitHub Actions secrets. The corresponding public key is embedded in Daystrom. Neither the private key nor an unprotected derivative may be uploaded as an artifact.

GitHub stores them as `TAURI_SIGNING_PRIVATE_KEY` and `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`. The password-protected private key is backed up separately from its password. Release builds fail when either updater secret or the existing platform-signing credentials are unavailable.

On Windows, Authenticode signing changes the NSIS installer bytes after Tauri has bundled them. Release automation therefore regenerates the detached Tauri signature after Authenticode signing and uploads only that final signature.

Daystrom accepts an update or rollback package only when all of the following hold:

- its Tauri signature validates against the embedded public key;
- its version and platform match the signed metadata;
- its source is the configured Daystrom update service;
- a rollback targets exactly the recorded predecessor of the installed version.

Signing-key rotation requires an explicit migration release trusted by the previous key. It must not be an unplanned CI secret replacement.

## Update discovery and user experience

Daystrom checks for application updates independently from the 30-minute Scopely game-version check. Initially, application updates use only the stable public GitHub release channel at `https://github.com/MBurchard/project-daystrom/releases/latest/download/latest.json`.

The Rust backend exclusively owns update discovery, downloads, signature verification, predecessor retention, installation, and rollback state. The frontend receives domain state and progress and invokes only narrowly scoped Daystrom commands. It does not use the JavaScript updater plugin and is not granted Tauri's general updater capability. Every frontend request is validated against the update and rollback invariants by the backend before it can mutate application state.

The intended cadence is:

- once after normal application startup;
- every six hours while Daystrom remains active;
- immediately when the user selects `Check for Daystrom updates`.

Development builds do not install production updates.

When an update is available, Daystrom shows it in the main window and may issue a native notification. A notification click brings Daystrom to the foreground. It never closes the game, starts the Scopely launcher, or installs the update without confirmation.

The user can choose `Install update` or `Later`. There is no forced background installation in the initial implementation. Download progress and actionable errors remain visible in Daystrom.

## Installation while the game is running

Installing a Daystrom update requires Daystrom to restart, but must not terminate STFC automatically. The running game keeps the mod version already loaded in its process. The mod reconnects to the restarted Daystrom backend through the existing resilient WebSocket discovery mechanism.

The newly bundled mod becomes effective on a later game start and deployment. If an update requires the game to be closed for a mod change, Daystrom explains that requirement and waits for the user; it does not kill the game process.

Before handing control to the platform updater, Daystrom flushes persistent settings and closes logging with the existing graceful-shutdown path.

## Retaining one rollback release

Daystrom retains a verified updater package for exactly the version installed immediately before the latest successful update. It does not copy the live application directory. Keeping an original signed release package is more predictable across NSIS installations and signed macOS app bundles.

Before updating from version A to version B, Daystrom must have:

- a completely downloaded and verified update package for version B;
- a completely downloaded and verified rollback package for the currently installed version A;
- signed metadata recording A as B's only allowed rollback target;
- a one-generation backup of settings needed to recover from an incompatible migration.

Only after those prerequisites succeed may installation of B begin. The packages and metadata are stored in the platform application-data directory rather than the installation directory.

After B starts successfully, A remains available until the next update. Before an update from B to C, the old A package is removed only after the verified B rollback package is safely stored. At steady state there is never more than one rollback release.

The initial updater-enabled release must publish a retrievable updater package for itself so that it can be retained before installing its successor.

Version `0.9.0` is the first updater-enabled release and has not previously been distributed. The first end-to-end update test will install `0.10.0` over `0.9.0`; the project may move to `1.0.0` after that validation.

## One-click rollback

When a rollback package is available, diagnostics expose:

`Restore previous Daystrom version <version>`

The action shows the target version and explains that it restores Daystrom and its bundled mod, not STFC. The rollback flow then:

1. verifies the cached package, signature, version, platform, and predecessor metadata again;
2. persists current settings and creates the one-generation settings backup;
3. asks the user to close the game when restoring the bundled mod requires it;
4. never terminates the game automatically;
5. closes logging and starts a narrowly scoped external restore process;
6. exits Daystrom;
7. installs the cached predecessor package;
8. restarts the restored Daystrom version;
9. marks the rejected version so it is not offered again automatically in the same process or without an explicit user decision.

The restore process accepts only the already verified local package and signed metadata. It must not accept arbitrary URLs, versions, or installation paths. It waits for the Daystrom process to exit before replacing files. Its exact implementation may reuse supported Tauri updater primitives or be a small platform helper, but it must preserve Windows installer semantics and macOS signing and notarization.

Rolling back Daystrom does not rewrite a mod already loaded into a running game process. The restored bundled mod takes effect after the game is closed and the previous mod is deployed for the next start.

## Settings and profile compatibility

Every release must remain capable of reading settings and profiles written by its immediate successor. This one-version compatibility window is part of the rollback contract.

Changes to persisted data therefore need one of these strategies:

- additive fields with defaults and tolerant deserialization;
- a reversible migration;
- preservation of the previous representation until the rollback window closes;
- restoration from the one-generation settings backup.

Destructive or irreversible migrations block one-click rollback and require an explicit architecture decision before release. Game profiles are user data and must never be silently discarded during update or rollback.

## Failure recovery

The normal one-click rollback assumes the updated Daystrom application still starts. The implementation must also provide a documented recovery path for a version that cannot launch at all.

The preferred design is a separately launchable, signed restore entry point that can consume only the cached predecessor package and metadata. Until that path exists and is tested on both platforms, the published previous installer and a documented manual reinstall procedure remain the fallback.

The update cache must tolerate interrupted downloads, power loss, and a failed install. Temporary packages are never promoted to trusted rollback state until their download and signature verification complete.

## Verification gates

Release preparation must fail unless all existing lint, typecheck, backend, frontend, and mod checks pass. The existing cross-platform game compatibility gate remains mandatory and independent from updater checks.

Before publishing a draft, manually verify at least:

- the Windows installer is signed and installs over the previous stable version;
- the universal macOS application is signed, notarized, and installs over the previous stable version;
- both builds discover the draft candidate only through an explicit test setup, never through the stable production endpoint;
- update artifacts and `latest.json` signatures validate;
- a production client does not see the draft release;
- publishing makes the release discoverable through the stable endpoint;
- Daystrom updates and reconnects without terminating a running game;
- the previous Daystrom version can be restored on Windows and macOS;
- the restored version can read settings and profiles written by its successor;
- a dismissed, denied, interrupted, malformed, or tampered update fails safely;
- only one predecessor package remains after consecutive updates.

The first updater release also requires a controlled end-to-end test from the immediately previous stable version after publication. A separate opt-in test channel may be added later for full pre-publication updater tests.

## Planned implementation slices

1. Convert the GitHub release workflow to draft preparation plus manual publication.
2. Add Tauri updater artifacts, signing secrets, signature verification, and `latest.json` generation.
3. Add update discovery and the non-forced Daystrom update UI.
4. Integrate graceful shutdown, installation, restart, and game reconnection.
5. Add predecessor-package retention and signed rollback metadata.
6. Add the one-click restore process and one-generation settings recovery.
7. Add cross-platform integration tests and the non-starting-application recovery path.

Each slice should remain independently reviewable. Auto-update must not be enabled for production clients until the signing, draft-release, and cross-platform recovery paths are all validated.
