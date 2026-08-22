# Project Daystrom ![Version](https://img.shields.io/github/v/release/MBurchard/project-daystrom?color=4488FF&label=)

![Crafted with Rust](https://img.shields.io/badge/Crafted_with-Rust-000000?logo=rust&logoColor=white)
![Crafted with TypeScript](https://img.shields.io/badge/Crafted_with-TypeScript-3178C6?logo=typescript&logoColor=white)
[![License: GPL v3][license-badge]][license]
[![CI][ci-badge]][ci]

[🇬🇧 English](README.md) | 🇩🇪 Deutsch

Eine Companion-App und ein Game-Mod für
[Star Trek Fleet Command](https://www.scopely.com/games/star-trek-fleet-command) auf macOS und Windows.

## Was ist das?

Project Daystrom ist eine native Desktop-App mit integriertem Game-Mod für STFC. Der Mod ist vollständig in Rust
geschrieben und nutzt eine eigene Hook-Engine (ARM64 + x86_64), die direkt in die IL2CPP-Laufzeitumgebung des
Spiels eingreift.

**Hauptfunktionen:**

- **Multi-Account-Unterstützung** auf Windows und macOS
  - Jeder Account bekommt ein eigenes TOML-basiertes Profil mit isolierten Spieleinstellungen
  - Ein eigener PlayerPrefs-Interceptor leitet Spieleinstellungen in profilspezifischen Speicher um
  - Eigene Account-Tabs trennen Auswahl, Start und Laufstatus
  - Lokale Profile und Anmeldedaten lassen sich mit ausdrücklicher Bestätigung sicher löschen
  - Profile sind plattformübergreifend portabel
- **Spielverbesserungen** durch den Rust-Mod
  - Konfigurierbare Tastenkürzel mit Konflikterkennung gegen Spiel-Keybindings
  - Einstellbare UI-Skalierung (50–200 %) wird live auf das laufende Spiel angewendet
  - Automatisches Öffnen der Chat-Sidebar beim Spielstart
  - Automatisches Aufklappen des Auftragsqueue-Panels beim Spielstart
  - Konfigurierbarer Systemansicht-Zoom und Schiffsnamen-Sichtbarkeitsreichweite
  - Automatisches Öffnen der Cargo-Ansicht für Hostiles, Armadas, Stationen und Spielerschiffe
  - Lootbox-Öffnungsanimation überspringen (standardmäßig aktiviert)
  - Erstes Popup nach Spielstart überspringen (standardmäßig aktiviert)
  - Ein-Tasten-Kampfablauf: Main-Action-Taste wiederholt drücken, um das nächste erreichbare Hostile auszuwählen und
    ohne Maus anzugreifen
  - Konfigurierbarer Main-Action-Shortcut mit Unterstützung für Tastaturtasten und zusätzliche Maustasten
  - Toast-Banner-Unterdrückung mit typ-basiertem Opt-out (Kampf, Station, Armada etc.)
  - Konfigurierbare Slider-Limits für Standardrekrutierung, Allianzspenden und Transportermuster
  - Shop- und Raffineriepakete bleiben während ihrer Abklingzeit einsehbar
  - Automatische Erkennung von STFC-Updates über die Scopely-Update-API
- **Native plattformübergreifende App** (Tauri 2 + Vue 3)
  - Deutsche und englische Benutzeroberfläche
  - Einstellbare Skalierung der Daystrom-Oberfläche über Tastatur oder Mausrad
  - Einheitlicher Launcher: Entitlement-Patching auf macOS, DLL-Proxy-Injection auf Windows
  - Prozessüberwachung mit automatischer Erkennung von Spiel- und Launcher-Aktivität
  - Sichtbare Warnung, wenn STFC ohne verbundenen Daystrom-Mod läuft
  - System-Tray-Integration mit Minimize-to-Tray und Quit-Schutz
  - Live-WebSocket-Bridge, die Einstellungen in Echtzeit mit dem laufenden Spiel synchronisiert
  - Signierte Daystrom-Updates mit bewusster Installation, gestaffelter Freigabe und lokalisierten Release Notes
  - One-Click-Rollback zum verifizierten Vorgänger einschließlich Mod und Einstellungen

## Sicherheit und Accountschutz

Project Daystrom ist ein inoffizielles Community-Projekt und wird weder von Scopely entwickelt noch unterstützt. Es
verändert das Spiel und lädt eigenen Code in den laufenden Spielprozess. Spielupdates können Daystrom deshalb
vorübergehend inkompatibel machen; die Nutzung erfolgt auf eigene Verantwortung.

Verknüpfe jeden Spielaccount vor der Nutzung von Daystrom mit einer Scopely ID, damit der Zugang nicht von lokalen
Profilen oder Anmeldedaten abhängt. Daystrom zeigt bei der ersten Nutzung die vollständigen plattformspezifischen
Sicherheits- und Entfernungshinweise an und macht sie anschließend weiterhin über die Einstellungen zugänglich.

## Installation

Das neueste Release für deine Plattform findest du auf der
[Releases-Seite](https://github.com/MBurchard/project-daystrom/releases/latest).

- **macOS**: Lade die `.dmg`-Datei herunter, öffne sie und ziehe die App in den Programme-Ordner.
- **Windows**: Lade den `.exe`-Installer herunter und führe ihn aus. Falls Windows SmartScreen eine Warnung
  anzeigt, klicke auf "Weitere Informationen" und dann "Trotzdem ausführen" (die App ist selbstsigniert,
  noch nicht von Microsoft verifiziert).

Starte Project Daystrom nach der Installation, lies die Sicherheitshinweise und bereite den Mod über die Aktion in der
Statusleiste vor. Um den ersten Account hinzuzufügen, starte STFC über Daystrom und melde Dich beim gewünschten
Scopely-Account an. Daystrom fügt das Spielerprofil anschließend als Account-Tab hinzu. Für spätere Starts
verwendest Du dort `Starten`.

Daystrom zeigt lokalisierte Release Notes für signierte Anwendungsupdates an und installiert sie nur nach Bestätigung.
Ein laufendes Spiel bleibt während des Daystrom-Updates geöffnet; sein Mod verbindet sich nach dem Neustart von
Daystrom automatisch erneut. Sobald ein verifizierter Vorgänger verfügbar ist, bietet dasselbe Fenster einen
One-Click-Rollback an.

## Danksagung

Dieses Projekt wurde ursprünglich durch den [STFC Community Mod](https://github.com/netniV/stfc-mod) von
[netniV](https://github.com/netniV), [tashcan](https://github.com/tashcan) und weiteren Mitwirkenden inspiriert.
Daystrom nutzt inzwischen einen eigenen Rust-basierten Mod mit eigener Hook-Engine und Profilsystem.

## Gebaut mit

- [Tauri 2](https://tauri.app/) (Rust-Backend + native Shell)
- [Vue 3](https://vuejs.org/) + [Vite](https://vite.dev/) (Frontend)
- [@mburchard/bit-log](https://www.npmjs.com/package/@mburchard/bit-log) (strukturiertes Logging)
- Eigene IL2CPP-Hook-Engine in Rust (ARM64 + x86_64)

## Projektstruktur

```text
project-daystrom/
├── package.json            # Workspace-Root (orchestrierende Scripts)
├── pnpm-workspace.yaml     # Workspace-Konfiguration (members: app, scripts)
├── eslint.config.js        # Gemeinsame ESLint-Konfiguration (lintet das gesamte Projekt)
├── tsconfig.base.json      # Gemeinsame TypeScript-Basiskonfiguration
├── scripts/                # Build- und Tooling-Scripts
│   ├── build.ts            #   Mod- + App-Build-Orchestrierung
│   ├── update-manifest.ts  #   Release Notes + Updater-Manifest-Generierung
│   └── package.json        #   Script-Abhängigkeiten
├── release-notes/          # Versionierte deutsche und englische Release Notes
├── rust-mod/               # Daystrom Game-Mod (Rust, cdylib)
│   ├── src/hook/           #   Hook-Engine (Inline-Hooks, ARM64 + x86_64)
│   ├── src/hooks/          #   IL2CPP-Hook-Implementierungen
│   ├── src/il2cpp/         #   IL2CPP-Laufzeitumgebung-Bindings
│   └── Cargo.toml          #   Crate-Konfiguration
├── app/                    # Project Daystrom App (Tauri 2 + Vue 3)
│   ├── modules/
│   │   ├── app/            #   Vue 3 Frontend
│   │   ├── backend/        #   Tauri/Rust-Backend
│   │   └── plugins/        #   Feature-Plugins (Dashboard, Alerts, Advisor)
│   ├── resources/          #   Gemeinsame Assets (Logo, Icons)
│   └── package.json        #   App-Abhängigkeiten + App-lokale Scripts
└── README.md
```

## Voraussetzungen

- [Node.js](https://nodejs.org/) >= 24
- [pnpm](https://pnpm.io/) >= 11
- [Rust](https://www.rust-lang.org/tools/install) (stable)

### macOS

- **Apple Silicon**: Lokale Entwicklung setzt arm64 voraus; die CI erstellt universelle Builds
- Xcode Command Line Tools (`xcode-select --install`)

### Windows

- **Visual Studio Build Tools 2022** (oder VS Community): Workload "Desktopentwicklung mit C++",
  einschließlich eines **Windows SDK** (wird standardmäßig nicht mitinstalliert!)
- Rust: Standardinstallation über [rustup-init.exe](https://rustup.rs/) (Option 1 wählt die MSVC-Toolchain)

## Einrichtung

```sh
nvm use
pnpm install
```

Alle Befehle werden vom **Workspace-Root** ausgeführt, sofern nicht anders angegeben.

## Mod bauen

Der Rust-Mod in `rust-mod/` erzeugt eine Shared Library, die beim Start ins Spiel injiziert wird.

```sh
pnpm build:mod
```

Dies kompiliert den Mod für die aktuelle Plattform und kopiert das Ergebnis nach `app/resources/mod/`.

## Scripts

### Workspace-Root (vom Projektverzeichnis aus ausführen)

| Script                                     | Beschreibung                                                |
|--------------------------------------------|-------------------------------------------------------------|
| `pnpm install:all`                         | Alle Workspace-Abhängigkeiten forciert installieren         |
| `pnpm lint`                                | ESLint über das gesamte Projekt ausführen                   |
| `pnpm lint:fix`                            | ESLint mit automatischer Korrektur ausführen                |
| `pnpm typecheck`                           | TypeScript- + Rust-Typprüfungen                             |
| `pnpm test`                                | Tooling-, Mod-, Frontend- und Backend-Tests ausführen       |
| `pnpm test:app`                            | Alle App-Tests ausführen (Frontend + Backend)               |
| `pnpm test:app:frontend`                   | Nur Frontend-Tests ausführen (vitest)                       |
| `pnpm test:app:backend`                    | Nur Backend-Tests ausführen (cargo test + ts-rs)            |
| `pnpm test:app:frontend:watch`             | Frontend-Tests im Watch-Modus ausführen                     |
| `pnpm test:app:frontend:coverage`          | Frontend-Tests mit v8-Coverage ausführen                    |
| `pnpm test:app:backend:coverage`           | Backend-Tests mit llvm-cov-Coverage ausführen               |
| `pnpm check:mod:dump -- <Pfade>`           | IL2CPP-Dumps gegen das Kompatibilitätsmanifest prüfen       |
| `pnpm release:verify -- <macOS> <Windows>` | Kompatible Plattform-Dumps vor Releases voraussetzen        |
| `pnpm build`                               | Alles bauen (Mod-Bibliothek → Tauri-App)                    |
| `pnpm build:mod`                           | Mod-Bibliothek bauen und nach `app/resources/mod/` kopieren |
| `pnpm build:app`                           | Mod-Bibliothek + Tauri-App-Bundle bauen                     |
| `pnpm icons`                               | Tauri-Icons aus `resources/daystrom.png` generieren         |
| `pnpm dev`                                 | Mod bauen + Tauri-App mit Hot Reload starten                |

### Pfad-Aliase

| Alias          | Auflösung                     |
|----------------|-------------------------------|
| `@app/*`       | `modules/app/src/*`           |
| `@generated/*` | `modules/app/src/generated/*` |
| `@resources/*` | `resources/*`                 |

## Release-Wartung

Updaterfähige Releases benötigen Windows-, Apple- und Tauri-Signing-Zugangsdaten.
[release-signing.md](docs/release-signing.md) beschreibt deren Einrichtung; [releasing.md](docs/releasing.md) den
Release-Ablauf und die Kompatibilitätsregeln für das Manifest.

Jedes Release benötigt eine geprüfte `release-notes/<version>.json` mit deutschen und englischen Einträgen. Die
Release-Automatisierung validiert diese Datei und verwendet sie sowohl für den GitHub-Release-Text als auch für die in
`latest.json` eingebetteten Hinweise.

## App (Tauri + Vue 3 + Vite)

### Typgenerierung (ts-rs)

Gemeinsame Typen zwischen Rust-Backend und TypeScript-Frontend werden automatisch von
[ts-rs](https://github.com/Aleph-Alpha/ts-rs) generiert. Rust-Structs mit `#[derive(TS)]` erzeugen
TypeScript-Typen in `app/modules/app/src/generated/`, sobald `pnpm test:app:backend` ausgeführt wird.
Rust-Dokumentationskommentare werden als JSDoc übernommen.

```rust
#[derive(Serialize, TS)]
#[ts(export)]
pub struct GameStatus { /* ... */ }
```

```typescript
import type {GameStatus} from '@generated/GameStatus';
```
Plugins befinden sich in `modules/plugins/` und werden von der Haupt-App geladen. Die Architektur ist bewusst modular
gehalten, damit einzelne Plugins unabhängig entwickelt und gepflegt werden können.

### Umgebungsvariablen

| Variable                           | Standard                | Beschreibung                          |
|------------------------------------|-------------------------|---------------------------------------|
| `DAYSTROM_UPDATE_ENDPOINT`         | Konfigurierter Endpunkt | Nur Debug: Manifest-URL überschreiben |
| `DAYSTROM_UPDATE_INTERVAL_SECONDS` | `21600`                 | Nur Debug: Prüfintervall setzen       |

## Lizenz

Dieses Projekt steht unter der [GNU General Public License v3.0](https://www.gnu.org/licenses/gpl-3.0.html).

[license-badge]: https://img.shields.io/badge/License-GPLv3-blue.svg?logo=gnu&logoColor=white
[license]: https://www.gnu.org/licenses/gpl-3.0
[ci-badge]: https://github.com/MBurchard/project-daystrom/actions/workflows/ci.yml/badge.svg
[ci]: https://github.com/MBurchard/project-daystrom/actions/workflows/ci.yml
