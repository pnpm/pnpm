---
"@pnpm/pnpr": minor
---

A registry name in `pnpr.yaml` must now start with a letter and contain only letters, digits, `.`, `_`, and `-`. It cannot be a reserved pnpm prefix name such as `npm`, `jsr`, `git`, or `workspace`. pnpr refuses to start on a name that breaks these rules, so rename such a registry and update the `/~<name>/` URLs that clients use.
