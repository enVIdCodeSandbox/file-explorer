# Phones and tablets

FAST File Explorer builds for Android and for iPhone and iPad from the same
code as the desktop app. This page covers getting a package, installing it,
what works on each system, and how to look at the phone layout without a phone.

## Getting a package from GitHub

The mobile packages are built by `.github/workflows/mobile.yml`, which the **Build**
workflow runs on every push to `main` and on version tags. They are attached to the
same release as the desktop installers (the Releases page), as `FAST-File-Explorer-android.apk`
and `FAST-File-Explorer-unsigned.ipa`. A phone build that fails does not stop the release: it
is published without that file, with a warning in the run. You can also run the phone builds
alone by hand (**Actions > Mobile > Run workflow**); the run's page then lists two downloads
under **Artifacts**:

| Artifact | Contains | Install with |
| --- | --- | --- |
| `fast-file-explorer-android` | `FAST-File-Explorer-android.apk` (64-bit and 32-bit ARM) | Open the file on the phone |
| `fast-file-explorer-ios-unsigned` | `FAST-File-Explorer-unsigned.ipa` | A sideloading tool (below) |

The two jobs do not depend on each other, and neither holds up the desktop builds.

## Android

1. Copy the `.apk` to the phone and open it. Android asks once to allow
   installing from that source.
2. Start the app. It shows a card asking for **access to your files**. Tap the
   button and switch on *Allow access to manage all files*. A file manager
   cannot see shared storage without it; the app works with nothing else.

Needs Android 7 or newer.

### Signing

Android only installs signed packages, and only installs an update over an app
signed with the **same** key.

- **Without any setup**, the workflow makes a key for that one run. The package
  installs, but to install a later build you must uninstall the earlier one
  first (the app's settings are lost; your files are not touched).
- **With your own key**, updates install over each other. Make a key once:

  ```
  keytool -genkeypair -v -keystore release.jks -alias upload -keyalg RSA -keysize 2048 -validity 10000
  base64 -w0 release.jks        # macOS: base64 -i release.jks
  ```

  and add three repository secrets (**Settings > Secrets and variables > Actions**):

  | Secret | Value |
  | --- | --- |
  | `ANDROID_KEYSTORE` | the base64 text |
  | `ANDROID_KEYSTORE_PASSWORD` | the password you chose |
  | `ANDROID_KEY_ALIAS` | `upload` |

  Keep `release.jks` somewhere safe. Without it you cannot update the app.

## iPhone and iPad

The workflow builds **without an Apple developer account**, so the `.ipa` is
unsigned. iOS does not install unsigned apps, so one more step is needed:

- **Sideloadly** or **AltStore** (Windows or Mac) sign the `.ipa` with a free
  Apple ID while installing it. A free Apple ID's signature lasts **7 days**;
  the tool can renew it. A paid developer account lasts a year.
- On the device, trust the certificate: **Settings > General > VPN & Device
  Management**.

What an iPhone allows a file manager is narrow: an app sees **its own folder**
(shown in the Files app under *On My iPhone > FAST File Explorer*, so you can
move things in and out there) and whatever it reaches over the network. See
the table below.

## What works where

| | Android | iPhone / iPad |
| --- | --- | --- |
| Browse, copy, move, rename, delete | All of shared storage and memory cards | The app's own folder |
| Servers (FTP, SFTP, WebDAV) | Yes | Yes |
| Archives (open, extract, add, edit) | Yes | Yes |
| Search, folder sizes, storage analysis | Yes | Yes (within the app's folder) |
| Open a file in another app | Yes (the system's "open with" sheet) | Not yet: pictures, video, audio, PDF and text open in the app's own viewer |
| Opened by other apps | Offered for folders, "Open downloads" and "Open with"; choose "Always" to make it the default | Listed under Share > "Open in" |
| Smart Clear | Shared storage (not other apps' caches) | The app's own folder |
| Share sheet | Yes | Not yet (sharing by link on the local network works) |
| Recycle bin | No: deleting is permanent and always asks | No: same |
| Several windows, tabs dragged out, drag and drop between apps | No | No |
| System clipboard for files | No (copy and move inside the app work) | No |

## The three looks

On a phone the interface style is one of three arrangements, each like the
file manager that comes with a phone. The first-run setup recommends the one
that matches the device; change it in **Settings > Presets and profiles**.

- **Google**: like Files by Google. A search bar on top, Recents and Browse
  along the bottom, a menu button on every row.
- **Samsung**: like My Files. One home page with a large title, round category
  buttons and rounded cards; the path across the top of every folder.
- **Apple**: like Files. Large titles, grouped lists, a back button that names
  where you came from.

In all three: tap to open; press and hold to start selecting; Move, Copy,
Share, Delete and More appear along the bottom; after Move or Copy, open the
destination and tap **Move here** or **Copy here**.

## Performance on a phone

Phones get their own six presets (Essential, Light, Balanced, Smooth, Fast,
Maximum), far lighter than the computer ones: the heaviest uses less than
"Average" does on a computer. Every one of them slows down on battery and gives
memory back when the app is in the background. The setup recommends one from
the phone's memory and processor.

## Looking at the phone layout without a phone

- **In the desktop app**: Settings > Advanced > *Phone layout* > **Always**,
  then make the window narrow. Pick a phone look under Presets and profiles.
- **In a browser, with a pretend phone behind it** (no real files are touched):

  ```
  npm run dev
  ```

  then open `http://localhost:1420/preview.html?look=samsung` (or `google`,
  `apple`; add `&theme=light`, `&setup=1`, or `&open=/storage/emulated/0/Download`).

## Building on your own machine

Android (any OS; needs Android Studio or the command-line SDK with an NDK, and
`ANDROID_HOME` and `NDK_HOME` set):

```
rustup target add aarch64-linux-android armv7-linux-androideabi
npm run android:init        # generates src-tauri/gen/android and applies this app's additions
npm run android:dev         # run on a connected phone or emulator
npm run android:build       # make an .apk
```

iOS (a Mac with Xcode):

```
rustup target add aarch64-apple-ios aarch64-apple-ios-sim
npm run ios:init
npm run ios:dev             # the simulator needs no account
npm run ios:build:unsigned  # an unsigned .ipa, no account
```

`src-tauri/gen` is not kept in git. What this app adds to the generated
projects lives in `src-tauri/mobile` and is applied by
`scripts/mobile-setup.mjs`: on Android the storage permissions, the activity
that asks for file access and opens and shares files, and release signing from
`gen/android/keystore.properties`; on iOS the settings that show the app's
folder in the Files app.

## When the Android build cannot download Gradle's tools (0.24.1)

A build that stops with *Could not find org.jetbrains.kotlin:kotlin-stdlib…* (or another Kotlin file) while configuring `:buildSrc` failed before any of this app's code was built: Gradle could not fetch its own build-script tools from the Gradle plugin portal. `scripts/mobile-setup.mjs android` now gives `buildSrc` a `settings.gradle.kts` that also looks on Maven Central, and the Mobile workflow tries the Android build up to three times. Between tries it clears what Gradle has remembered as "not found" (Gradle keeps that for a day, so an unchanged second try used to fail at once without asking again) and prints what Maven Central answers at that moment.
