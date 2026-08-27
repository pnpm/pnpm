---
"@pnpm/installing.env-installer": patch
"pacquet": patch
"pnpm": patch
---

A `devEngines.packageManager` range pin on pnpm is now recorded in `pnpm-lock.yaml`'s `packageManagerDependencies` when the running pnpm already satisfies it, using the running version and keeping the range as the recorded specifier. Previously only an exact pin — or a range resolved on the way through a version switch — reached the lockfile, so a range pin written by hand (or by any tool other than `pnpm add` / `pnpm self-update`) left the project without the shared resolution the pin exists to provide.
