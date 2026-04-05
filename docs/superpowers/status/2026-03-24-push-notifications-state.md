# Push Notifications Status

Date: 2026-03-24

## Goal

Server-side spa heat push notifications, with minimal client work.

## Current Server/Firebase State

- Firebase project in use: `pool-eb7ed`
- Android Firebase app created for `com.ssilver.pentair`
- iOS Firebase app created for `com.ssilver.pentair.ios`
- Real Android config file present:
  - [google-services.json](pentair-android/app/google-services.json)
- Real iOS config file present:
  - [GoogleService-Info.plist](pentair-ios/PentairIOS/GoogleService-Info.plist)
- Daemon FCM service account key exists locally at:
  - `~/.pentair/firebase/pool-eb7ed-pentair-daemon-fcm.json`
- Live daemon config file points at that Firebase project/key:
  - `/tmp/pentair-daemon-8080.toml`

## Apple/Firebase Console State

- User created APNs auth key and provided:
  - key file on Mac: `~/Downloads/AuthKey_CMMDZ3CMW8.p8`
  - key id: `CMMDZ3CMW8`
  - team id: `M8B368H9T5`
- User reported the APNs auth key has been uploaded in Firebase Console for the iOS app.

## Android State

- Server-side push policy code is implemented in daemon but not committed yet.
- Android app has real Firebase config.
- Android token registration was tightened so the app does not rely only on `onNewToken`.
- Relevant Android files:
  - [DeviceTokenManager.kt](pentair-android/app/src/main/java/com/ssilver/pentair/data/DeviceTokenManager.kt)
  - [MessagingTokenProvider.kt](pentair-android/app/src/main/java/com/ssilver/pentair/data/MessagingTokenProvider.kt)
  - [DeviceRegistrationClient.kt](pentair-android/app/src/main/java/com/ssilver/pentair/data/DeviceRegistrationClient.kt)
  - [DaemonDiscovery.kt](pentair-android/app/src/main/java/com/ssilver/pentair/discovery/DaemonDiscovery.kt)
  - [PoolApp.kt](pentair-android/app/src/main/java/com/ssilver/pentair/PoolApp.kt)
  - [DeviceTokenManagerTest.kt](pentair-android/app/src/test/java/com/ssilver/pentair/data/DeviceTokenManagerTest.kt)
- Verified earlier in this session:
  - `./gradlew app:testDebugUnitTest app:assembleDebug` passed
  - latest Android build was installed to the Pixel and launched
- Not yet verified:
  - device token registration back to daemon
- Current evidence:
  - `~/.pentair/devices.json` does not exist yet

## iOS State

- Minimal iOS push wiring is partially implemented but not verified yet.
- New iOS files added:
  - [NotificationTokenManager.swift](pentair-ios/PentairIOS/NotificationTokenManager.swift)
  - [PoolAppDelegate.swift](pentair-ios/PentairIOS/PoolAppDelegate.swift)
  - [PentairIOS.entitlements](pentair-ios/PentairIOS/PentairIOS.entitlements)
  - [NotificationTokenManagerTests.swift](pentair-ios/Tests/NotificationTokenManagerTests.swift)
- Existing iOS files modified:
  - [PentairIOSApp.swift](pentair-ios/PentairIOS/PentairIOSApp.swift)
  - [PoolViewModel.swift](pentair-ios/PentairIOS/PoolViewModel.swift)
  - [Info.plist](pentair-ios/PentairIOS/Info.plist)
  - [project.pbxproj](pentair-ios/PentairIOS.xcodeproj/project.pbxproj)
- Token-manager test passed on the Mac toolchain:
  - compiled `NotificationTokenManager.swift` + test harness with `swiftc`
  - ran `/tmp/notification-token-manager-tests`
- Xcode project/package wiring is not verified yet.

## Current Blocker

The live daemon is not staying up.

Observed current state:
- no daemon process running
- nothing listening on `:8080`
- latest daemon log at [/tmp/pentair-daemon-8080.log](/tmp/pentair-daemon-8080.log) shows only normal startup through:
  - bind
  - FCM configured
  - adapter discovered/connected
- after that, the process exits immediately with no visible crash output in the log

This blocks:
- verifying Android device token registration
- verifying iOS token registration
- end-to-end push testing

## Uncommitted Repo State

Current `git status` includes:

- daemon push-notification work
- Android Firebase/token-registration work
- iOS Firebase/push-registration work
- generated Firebase config files
- local experimental heat-estimator history file dirtied again

Notable uncommitted files:
- [pentair-daemon/src/spa_notifications.rs](pentair-daemon/src/spa_notifications.rs)
- [pentair-ios/PentairIOS/GoogleService-Info.plist](pentair-ios/PentairIOS/GoogleService-Info.plist)
- [pentair-ios/PentairIOS/NotificationTokenManager.swift](pentair-ios/PentairIOS/NotificationTokenManager.swift)
- [pentair-ios/PentairIOS/PoolAppDelegate.swift](pentair-ios/PentairIOS/PoolAppDelegate.swift)
- [pentair-ios/PentairIOS/PentairIOS.entitlements](pentair-ios/PentairIOS/PentairIOS.entitlements)

## Next Steps

1. Debug why `pentair-daemon` exits immediately after startup when run with the current config.
2. Once daemon stays up:
   - relaunch Android app and confirm `~/.pentair/devices.json` appears
   - verify FCM send path with a test notification
3. Resolve Firebase Swift package on the Mac and complete an iOS simulator build.
4. Install/run the iOS app on device and confirm token registration to daemon.
5. Only then commit the daemon/Android/iOS push work.

## Useful Commands

Check daemon health:

```bash
ps -ef | grep 'target/release/pentair-daemon' | grep -v grep
ss -ltnp '( sport = :8080 )'
tail -n 80 /tmp/pentair-daemon-8080.log
```

Start daemon with current live config:

```bash
PENTAIR_CONFIG=/tmp/pentair-daemon-8080.toml target/release/pentair-daemon
```

Check Android token registration:

```bash
cat ~/.pentair/devices.json
```

Mac iOS build path:

```bash
ssh -i /tmp/pentair-mac -p 2222 ssilver@localhost \
  'cd /Users/ssilver/development/pentair/pentair-ios && xcodebuild -project PentairIOS.xcodeproj -scheme PentairIOS -destination "platform=iOS Simulator,name=iPhone 17 Pro" build'
```

## Mac Bridge / Deployment Notes

The Mac is reached through a reverse SSH tunnel from the Mac back to this Linux box.

### How to reach the Mac from Linux

Use:

```bash
ssh -i /tmp/pentair-mac -p 2222 ssilver@localhost
```

That assumes the Mac is running the reverse tunnel.

### Tunnel command that must be running on the Mac

```bash
ssh -NT \
  -o ExitOnForwardFailure=yes \
  -o ServerAliveInterval=30 \
  -o ServerAliveCountMax=3 \
  -R 2222:localhost:22 \
  ssilver@192.168.1.138
```

If that tunnel is killed, Linux can no longer drive Xcode or the Mac-side Android tooling.

### Important Mac paths

- repo checkout on Mac:
  - `/Users/ssilver/development/pentair`
- Android adb on Mac:
  - `~/Library/Android/sdk/platform-tools/adb`
- iOS project on Mac:
  - `/Users/ssilver/development/pentair/pentair-ios/PentairIOS.xcodeproj`

### Android deployment path

The Android phone is deployed through the Mac, not directly from Linux.

Typical flow:

```bash
rsync -az -e 'ssh -i /tmp/pentair-mac -p 2222' \
  pentair-android/app/build/outputs/apk/debug/app-debug.apk \
  ssilver@localhost:/Users/ssilver/development/pentair/pentair-android/app/build/outputs/apk/debug/app-debug.apk

ssh -i /tmp/pentair-mac -p 2222 ssilver@localhost '
  ADB=~/Library/Android/sdk/platform-tools/adb
  $ADB install -r /Users/ssilver/development/pentair/pentair-android/app/build/outputs/apk/debug/app-debug.apk
  $ADB shell am force-stop com.ssilver.pentair
  $ADB shell monkey -p com.ssilver.pentair -c android.intent.category.LAUNCHER 1
'
```

Check attached Android devices on the Mac:

```bash
ssh -i /tmp/pentair-mac -p 2222 ssilver@localhost \
  '~/Library/Android/sdk/platform-tools/adb devices -l'
```

### iOS deployment/build path

iOS builds also happen on the Mac.

Simulator build:

```bash
ssh -i /tmp/pentair-mac -p 2222 ssilver@localhost \
  'cd /Users/ssilver/development/pentair/pentair-ios && xcodebuild -project PentairIOS.xcodeproj -scheme PentairIOS -destination "platform=iOS Simulator,name=iPhone 17 Pro" build'
```

Direct CLI device deployment has historically been flaky because of Xcode signing/account state. The reliable fallback has been:

1. open the project on the Mac
2. select the real iPhone
3. hit Run once in Xcode

Current iOS push work also depends on the Mac because:
- `GoogleService-Info.plist` is already in the repo
- Xcode/SwiftPM still need to resolve Firebase packages on the Mac

### Current deployment status

- Android deployment to the Pixel through the Mac worked in this session.
- iOS push wiring is not yet fully built/verified on the Mac because the Firebase Swift package resolution/build step was still in progress when this status was written.
