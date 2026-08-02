# Project Daystrom ![Version](https://img.shields.io/github/v/release/MBurchard/project-daystrom?color=4488FF&label=)

![Crafted with Rust](https://img.shields.io/badge/Crafted_with-Rust-000000?logo=rust&logoColor=white)
![Crafted with TypeScript](https://img.shields.io/badge/Crafted_with-TypeScript-3178C6?logo=typescript&logoColor=white)
[![License: GPL v3](https://img.shields.io/badge/License-GPLv3-blue.svg?logo=gnu&logoColor=white)](https://www.gnu.org/licenses/gpl-3.0)
[![CI](https://github.com/MBurchard/project-daystrom/actions/workflows/ci.yml/badge.svg)](https://github.com/MBurchard/project-daystrom/actions/workflows/ci.yml)

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
  - Kontowechsel per Klick im Launcher, Profile sind plattformübergreifend portabel
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
  - Automatische Erkennung von Spiel-Updates über die Scopely-Update-API
- **Native plattformübergreifende App** (Tauri 2 + Vue 3)
  - Einheitlicher Launcher: Entitlement-Patching auf macOS, DLL-Proxy-Injection auf Windows
  - Prozessüberwachung mit automatischer Erkennung von Spiel- und Launcher-Aktivität
  - System-Tray-Integration mit Minimize-to-Tray und Quit-Schutz
  - Live-WebSocket-Bridge, die Einstellungen in Echtzeit mit dem laufenden Spiel synchronisiert

## Installation

Das neueste Release für deine Plattform findest du auf der
[Releases-Seite](https://github.com/MBurchard/project-daystrom/releases/latest).

- **macOS**: Lade die `.dmg`-Datei herunter, öffne sie und ziehe die App in den Programme-Ordner.
- **Windows**: Lade den `.exe`-Installer herunter und führe ihn aus. Falls Windows SmartScreen eine Warnung
  anzeigt, klicke auf "Weitere Informationen" und dann "Trotzdem ausführen" (die App ist selbstsigniert,
  noch nicht von Microsoft verifiziert).

Nach der Installation starte Project Daystrom und klicke auf den Play-Button, um das Spiel mit dem Mod zu starten.

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
│   └── package.json        #   Script-Abhängigkeiten
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
- [pnpm](https://pnpm.io/) >= 10
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

| Script                                     | Beschreibung                                           |
|--------------------------------------------|--------------------------------------------------------|
| `pnpm install:all`                         | Alle Workspace-Abhängigkeiten forciert installieren    |
| `pnpm lint`                                | ESLint über das gesamte Projekt ausführen              |
| `pnpm lint:fix`                            | ESLint mit automatischer Korrektur ausführen           |
| `pnpm typecheck`                           | TypeScript- + Rust-Typprüfungen                        |
| `pnpm test`                                | Alle Tests ausführen (Frontend + Backend)              |
| `pnpm test:app`                            | Alle App-Tests ausführen (Frontend + Backend)          |
| `pnpm test:app:frontend`                   | Nur Frontend-Tests ausführen (vitest)                  |
| `pnpm test:app:backend`                    | Nur Backend-Tests ausführen (cargo test + ts-rs)       |
| `pnpm test:app:frontend:watch`             | Frontend-Tests im Watch-Modus ausführen                |
| `pnpm test:app:frontend:coverage`          | Frontend-Tests mit v8-Coverage ausführen               |
| `pnpm test:app:backend:coverage`           | Backend-Tests mit llvm-cov-Coverage ausführen          |
| `pnpm check:mod:dump -- <Pfade>`           | IL2CPP-Dumps gegen das Kompatibilitätsmanifest prüfen  |
| `pnpm release:verify -- <macOS> <Windows>` | Kompatible Plattform-Dumps vor Releases voraussetzen   |
| `pnpm build`                               | Alles bauen (Mod-dylib → Tauri-App)                    |
| `pnpm build:mod`                           | Mod-dylib bauen und nach `app/resources/mod/` kopieren |
| `pnpm build:app`                           | Mod-dylib + Tauri-App-Bundle bauen                     |
| `pnpm icons`                               | Tauri-Icons aus `resources/daystrom.png` generieren    |
| `pnpm dev`                                 | Mod bauen + Tauri-App mit Hot Reload starten           |

### Pfad-Aliase

| Alias          | Auflösung                     |
|----------------|-------------------------------|
| `@app/*`       | `modules/app/src/*`           |
| `@generated/*` | `modules/app/src/generated/*` |
| `@resources/*` | `resources/*`                 |

## Windows Code Signing (optional)

Der GitHub-Workflow unterstützt optionales Code Signing für den NSIS-Installer. Dafür werden ein selbstsigniertes
Code-Signing-Zertifikat und zwei Repository-Secrets benötigt.

### Selbstsigniertes Zertifikat erstellen

```powershell
New-SelfSignedCertificate -Type CodeSigningCert -Subject "CN=Your Name, Code Signing" `
  -CertStoreLocation Cert:\CurrentUser\My -NotAfter (Get-Date).AddYears(5)
```

### Als PFX exportieren

Der Thumbprint wird beim Erstellen des Zertifikats angezeigt. Er lässt sich auch so finden:

```powershell
Get-ChildItem Cert:\CurrentUser\My -CodeSigningCert
```

```powershell
$cert = Get-ChildItem Cert:\CurrentUser\My\<thumbprint>
$pw = Read-Host -AsSecureString "PFX password"
Export-PfxCertificate -Cert $cert -FilePath daystrom.pfx -Password $pw
```

### PFX verifizieren

```powershell
certutil -dump daystrom.pfx
```

### GitHub Secrets konfigurieren

Die PFX-Datei als Base64 kodieren und in die Zwischenablage kopieren.\
Hinweis: PowerShell löst relative Pfade vom Home-Verzeichnis des Benutzers auf, nicht vom aktuellen Arbeitsverzeichnis.
Daher immer einen absoluten Pfad verwenden.\

```powershell
[Convert]::ToBase64String([IO.File]::ReadAllBytes("<path-to-pfx>")) | Set-Clipboard
```

Dann diese Repository-Secrets in GitHub setzen:

| Secret                 | Wert                                 |
|------------------------|--------------------------------------|
| `WINDOWS_PFX_BASE64`   | Base64-kodierte PFX (Zwischenablage) |
| `WINDOWS_PFX_PASSWORD` | Das Passwort vom Export              |

## App (Tauri + Vue 3 + Vite)

### Typgenerierung (ts-rs)

Gemeinsame Typen zwischen Rust-Backend und TypeScript-Frontend werden automatisch von
[ts-rs](https://github.com/Aleph-Alpha/ts-rs) generiert. Rust-Structs mit `#[derive(TS)]` erzeugen
TypeScript-Interfaces in `app/modules/app/src/generated/`, sobald `pnpm test:app:backend` ausgeführt wird.
Rust-Dokumentationskommentare werden als JSDoc übernommen.

```rust
#[derive(Serialize, TS)]
#[ts(export)]
pub struct GameStatus { /* ... */ }
```

```typescript
import type {GameStatus} from '@generated/GameStatus';
```


Plugins befinden sich in `modules/plugins/` und werden von der Haupt-App geladen. Die Architektur ist bewusst
modular gehalten, damit einzelne Plugins unabhängig entwickelt und veröffentlicht werden können.

### Umgebungsvariablen

| Variable            | Standard | Beschreibung                                              |
|---------------------|----------|-----------------------------------------------------------|
| `DAYSTROM_DEVTOOLS` | `1`      | Auf `0` setzen, um DevTools in Debug-Builds zu verstecken |

## Lizenz

Dieses Projekt steht unter der [GNU General Public License v3.0](https://www.gnu.org/licenses/gpl-3.0.html).
