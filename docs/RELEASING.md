# Releasing

Installers are built by `.github/workflows/build.yml` for Windows (x64, ARM64), macOS (universal) and Linux (x64, ARM64). Push a tag to get a draft GitHub release with all of them attached:

```
git tag v0.1.0 && git push origin v0.1.0
```

Each build is uploaded as an artifact named `fast-file-explorer-build-<os>` (for example `fast-file-explorer-build-windows-x64`). The word "installer" is left out of the name on purpose: Chrome blocks downloads whose names contain it.


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

**Without an Apple account (recommended minimum):** run `scripts/make-mac-signing-cert.sh` once. It makes a free self-signed certificate and prints the same three secrets (`APPLE_CERTIFICATE`, `APPLE_CERTIFICATE_PASSWORD`, `APPLE_SIGNING_IDENTITY`). Every build is then signed with the same identity. This matters: a Mac app built unsigned gets a new identity at every build, so macOS forgets Full Disk Access at each update, and an entry that looks switched on in System Settings belongs to the earlier build and does nothing. Keep the generated `.p12`: a new certificate is a new identity. It does not remove the "unidentified developer" warning (only a paid certificate does). Untested here: it has not been run through this workflow on a real runner.

### Windows

Needs a code signing certificate (OV or EV) as a `.pfx` file.

| Secret | What it is |
|---|---|
| `WINDOWS_CERTIFICATE` | The `.pfx`, encoded with `base64 -w0 cert.pfx` (or `[Convert]::ToBase64String` in PowerShell) |
| `WINDOWS_CERTIFICATE_PASSWORD` | Its password |

The workflow imports the certificate into the runner's store and builds with its thumbprint, SHA-256 and a timestamp from DigiCert, so signatures stay valid after the certificate expires. Certificates kept on hardware tokens or in a cloud HSM cannot be exported; for those, give Tauri a `bundle.windows.signCommand` instead (see the Tauri documentation on Windows code signing).

### Linux

Packages are not signed. AppImage signing with GPG and repository signing for `.deb`/`.rpm` are left for when there is a package repository.

## Releases

When everything on `main` has built, the workflow posts the installers on the Releases page as **FAST File Explorer \<version\>**: the version is the one in `package.json`, the description starts with the name and version and is followed by GitHub's own generated change list since the last release. It only posts when every required installer is there (Windows x64 `.exe` and `.msi`, Linux `.AppImage`, `.deb` and `.rpm`, macOS `.dmg`); the optional ARM builds are attached when they succeeded. A version that is already published is left alone, so **raise the version (`npm run bump`) with each update**, or nothing new appears. Pull requests and other branches never publish. The app's updater reads this page.

## Version numbers

Every update that is handed out raises the version, in step with how big it is:

```
npm run bump -- patch   # small fixes and tweaks   0.2.0 -> 0.2.1
npm run bump -- minor   # a feature or several     0.2.1 -> 0.3.0
npm run bump -- major   # a big change             0.3.0 -> 1.0.0
```

The script rewrites `Cargo.toml`, `src-tauri/tauri.conf.json`, `package.json`, `package-lock.json` and the two entries of `Cargo.lock` together.

## Automatic updates

The app updates itself from the builds this repository's `build.yml` workflow publishes as Actions artifacts (see *Updates* in the user guide). The artifact names (`fast-file-explorer-build-<platform>`) are what the app looks for, so renaming them in the workflow breaks updating. The workflow's `GITHUB_SHA` is compiled into each build; that is how a build knows which commit it came from.

These builds are **not signed**, so the app checks only the SHA-256 GitHub reports for the download. For signed, verified updates use the Tauri updater plugin instead: it needs a signing key pair (`tauri signer generate`) and somewhere to host the update manifest (GitHub Releases works). Do that after the installers are signed.

## Crash reports

Builds can send crash reports to the developer (only for people who turn on Settings > Advanced > Crash logs > "Send crash reports to the developer automatically", or press "Send to the developer" on one report). The report is posted into the project's Google Form ("Bug report form"; one paragraph question), with no account and no key. The form's address and the question's id (`entry.1859183185`) are constants at the top of `src-tauri/src/crash.rs`: if the form is replaced or its question is recreated, change them there (the id is in the form's page source, in the `FB_PUBLIC_LOAD_DATA_` list). The form must stay open to anyone without sign-in and must not collect e-mail addresses. Replies are in the form's Responses tab.
