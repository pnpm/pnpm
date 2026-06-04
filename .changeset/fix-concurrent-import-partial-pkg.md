---
"@pnpm/fs.indexed-pkg-importer": patch
"pnpm": patch
---

Fixed packages being materialized into the virtual store without their root-level files (`package.json`, `LICENSE`, README, root entrypoints) when multiple `pnpm install` processes ran against the same store/workspace concurrently. The fast import path used to destructively empty the shared target directory, so a concurrent importer could wipe files another importer had already written; if the surviving files included the `package.json` completion marker, every later install treated the broken directory as complete and never repaired it. The fast path now imports directly only when it can create the target directory exclusively, and otherwise builds the package in a private temp directory and atomically renames it into place [#12197](https://github.com/pnpm/pnpm/issues/12197).
