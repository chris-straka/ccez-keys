# Keyring: the owner's keys for their other apps

ccez-keys holds the owner's API keys by name (owner, 2026-11-16: "Keys
should store more than just deepseek key"): `deepseek`, and any other name
`keys key add <name>` on z sends to the phone (README, "Keys, once"), or
Settings → AI → Paste on a key's row. Settings → AI lists each key held.
The owner's other apps (ccez-tube, ccez-llm, ccez-translate…) ask it for a
key by name instead of each keeping a copy: one place to change it, and the phone
keeps its AI when the Mac is off. Decided 2026-11-13 with a second
opinion from another session; it's for convenience and changing the
key, not for containing a bad app (an allowed app holds the raw key in
memory while it uses it).

## Asking (the app's side)

```kotlin
contentResolver.call(Uri.parse("content://uk.ccez.keys.keyring"), "key", "deepseek", null)
```

The third argument is the key's name; none is `deepseek`.

Off the main thread. The answer's `status`:

- `ok`: `key` (String) and `version` (Int, bumped on every change to any key).
- `approve`: not decided yet. The app starts
  `uk.ccez.keys/uk.ccez.keys.keyring.Approve` **for a result** (a keyboard
  in the background may not open screens; the app on screen can). That
  screen reads who asked from the system, shows the app's name and
  certificate, and remembers the answer.
- `denied`: the owner said no (Settings → AI → Remove lets it ask again).
- `none`: ccez-keys holds no key by that name.
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
- An allowed app may ask for any name held: the owner's yes is per app,
  not per key.
- Logged: which app asked and the answer, never the key. The key isn't
  backed up (`allowBackup="false"`; a new phone gets it pasted again).

## From the Mac

`keys keyring` lists the apps decided; `keys keyring allow|deny|drop
<package>` decides one without the screen. `keys key ls` lists the keys
held (names only), `keys key rm <name>` removes one.

On the phone each key is encrypted with an Android Keystore key. DeepSeek's
stays in the `ai` pref where the one key always lived; another name is
`key:<name>` (`learn/Ai.kt`, `KeyNames`). The control point's `key` method
takes the name as its argument (none is `deepseek`), and `status` lists the
names held as `keys`.
