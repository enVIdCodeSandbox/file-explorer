# Releasing

Installers are built by `.github/workflows/build.yml` for Windows (x64, ARM64), macOS (universal) and Linux (x64, ARM64). Push a tag to get a draft GitHub release with all of them attached:

```
git tag v0.1.0 && git push origin v0.1.0
```

## Signing

Unsigned installers work, but Windows SmartScreen and macOS Gatekeeper warn about them. Signing needs certificates that only you can obtain, so the workflow is prepared but **has not been run with real certificates**. Add these as repository secrets (Settings → Secrets and variables → Actions); anything missing simply leaves that platform unsigned.

### macOS (signing and notarization)

Needs an Apple Developer account and a "Developer ID Application" certificate.

| Secret | What it is |
|---|---|
| `APPLE_CERTIFICATE` | The certificate exported from Keychain Access as `.p12`, encoded with `base64 -i cert.p12` |
| `APPLE_CERTIFICATE_PASSWORD` | The password chosen when exporting |
| `APPLE_SIGNING_IDENTITY` | Like `Developer ID Application: Your Name (TEAMID)` |
| `APPLE_ID`, `APPLE_PASSWORD`, `APPLE_TEAM_ID` | Your Apple ID, an app-specific password for it, and your team ID; with these the build also notarizes the app |

Tauri reads these variables itself during `tauri build`.

### Windows

Needs a code signing certificate (OV or EV) as a `.pfx` file.

| Secret | What it is |
|---|---|
| `WINDOWS_CERTIFICATE` | The `.pfx`, encoded with `base64 -w0 cert.pfx` (or `[Convert]::ToBase64String` in PowerShell) |
| `WINDOWS_CERTIFICATE_PASSWORD` | Its password |

The workflow imports the certificate into the runner's store and builds with its thumbprint, SHA-256 and a timestamp from DigiCert, so signatures stay valid after the certificate expires. Certificates kept on hardware tokens or in a cloud HSM cannot be exported; for those, give Tauri a `bundle.windows.signCommand` instead (see the Tauri documentation on Windows code signing).

### Linux

Packages are not signed. AppImage signing with GPG and repository signing for `.deb`/`.rpm` are left for when there is a package repository.

## Automatic updates

Not set up. It needs the Tauri updater plugin, a signing key pair for update files (`tauri signer generate`), and a place to host the update manifest (GitHub Releases works). Do this after the installers are signed, because the update check should only ever download builds you signed.
