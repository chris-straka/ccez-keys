# Keyring: the AI key for the owner's other apps

ccez-keys holds the AI key (Settings → AI, or `keys key`). The owner's
other apps (ccez-tube, ccez-llm, ccez-translate…) ask it for that key
instead of each keeping a copy: one place to change it, and the phone
keeps its AI when the Mac is off. Decided 2026-11-13 with a second
opinion from another session; it's for convenience and changing the
key, not for containing a bad app (an allowed app holds the raw key in
memory while it uses it).

## Asking (the app's side)

```kotlin
contentResolver.call(Uri.parse("content://uk.ccez.keys.keyring"), "key", "muse", null)
```

Off the main thread. The answer's `status`:

- `ok`: `key` (String) and `version` (Int, bumped on every change).
- `approve`: not decided yet. The app starts
  `uk.ccez.keys/uk.ccez.keys.keyring.Approve` **for a result** (a keyboard
  in the background may not open screens; the app on screen can). That
  screen reads who asked from the system, shows the app's name and
  certificate, and remembers the answer.
- `denied`: the owner said no (Settings → AI → Remove lets it ask again).
- `none`: ccez-keys has no key for that provider (its provider differs).
- `locked`: the phone hasn't been unlocked since it started.

An app with no ccez-keys gets an `IllegalArgumentException` (unknown
provider). The app needs `<queries><package android:name="uk.ccez.keys"/></queries>`.

Apps ask each time they need the key (a few ms) and keep it in memory
only, so a changed key reaches them at once. A copy to paste is
`ccez-tube/android/.../data/Keyring.kt`.

## How it decides

- The caller is the system's word: `Binder.getCallingUid()`, which must
  be one package (shared uids are refused) and match `callingPackage`.
  Nothing the caller sends names it.
- Allowing stores the app's current signing certificate (SHA-256);
  later asks must be signed by it (`hasSigningCertificate`, so a key
  rotation still matches). Another app under the same name gets nothing.
- No signature permission: the owner's apps are signed with different
  keys, and moving an installed app to a new key means reinstalling it.
- The provider serves `call("key")` only; query, insert and files answer
  nothing. The yes/no screen hides overlays and drops obscured taps.
- Logged: which app asked and the answer, never the key. The key isn't
  backed up (`allowBackup="false"`; a new phone gets it pasted again).

## From the Mac

`keys keyring` lists the apps decided; `keys keyring allow|deny|drop
<package>` decides one without the screen.
