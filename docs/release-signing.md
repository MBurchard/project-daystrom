# Release signing

Updater-enabled releases require platform signing for Windows and macOS plus the Tauri updater key. The release
workflows reject missing credentials before building distributable artefacts.

## Required GitHub secrets

| Purpose                     | Secret                               |
|-----------------------------|--------------------------------------|
| Tauri updater private key   | `TAURI_SIGNING_PRIVATE_KEY`          |
| Tauri updater key password  | `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` |
| Windows PFX certificate     | `WINDOWS_PFX_BASE64`                 |
| Windows PFX password        | `WINDOWS_PFX_PASSWORD`               |
| Apple signing certificate   | `APPLE_CERTIFICATE_BASE64`           |
| Apple certificate password  | `APPLE_CERTIFICATE_PASSWORD`         |
| Apple developer account     | `APPLE_ID`                           |
| Apple app-specific password | `APPLE_ID_PASSWORD`                  |
| Apple developer team        | `APPLE_TEAM_ID`                      |

Store backups of signing credentials separately from their passwords. Never commit private keys, certificates,
passwords, or unprotected derivatives.

## Tauri updater key

The public key is embedded in `app/modules/backend/tauri.conf.json`. Release packages and rollback metadata must be
signed by its matching private key. Losing the private key or password prevents existing installations from trusting
future updates; rotation therefore requires a migration release signed by the previous key.

The release process and manifest compatibility rules are documented in [releasing.md](releasing.md).

## Windows certificate

The current workflow accepts a password-protected PFX. A self-signed code-signing certificate can be created in
PowerShell:

```powershell
New-SelfSignedCertificate -Type CodeSigningCert -Subject "CN=Your Name, Code Signing" `
  -CertStoreLocation Cert:\CurrentUser\My -NotAfter (Get-Date).AddYears(5)
```

Find the certificate and export it:

```powershell
Get-ChildItem Cert:\CurrentUser\My -CodeSigningCert

$cert = Get-ChildItem Cert:\CurrentUser\My\<thumbprint>
$pw = Read-Host -AsSecureString "PFX password"
Export-PfxCertificate -Cert $cert -FilePath daystrom.pfx -Password $pw
```

Verify the exported file before storing it:

```powershell
certutil -dump daystrom.pfx
```

Encode the PFX from an absolute path and copy it to the clipboard:

```powershell
[Convert]::ToBase64String([IO.File]::ReadAllBytes("<absolute-path-to-pfx>")) | Set-Clipboard
```

Store the result as `WINDOWS_PFX_BASE64` and the export password as `WINDOWS_PFX_PASSWORD`.

## Apple credentials

`APPLE_CERTIFICATE_BASE64` contains the exported signing certificate encoded as Base64.
`APPLE_CERTIFICATE_PASSWORD` protects that export. Notarization uses the Apple ID, an app-specific password, and the
associated team ID.

The macOS release workflow imports the certificate into a temporary keychain, signs the universal application, submits
it for notarization, staples the result, and removes temporary credentials after the build.
