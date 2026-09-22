---
"pacquet": patch
---

Fixed `pnpm install` skipping optional dependencies that the running Node.js supports. When `devEngines.runtime` declares a range without `onFail: download`, pnpm now uses the Node.js already on the system. Only an entry that sets `onFail` to `download` makes the range's lower bound the version pnpm provisions [pnpm/pnpm#15230](https://github.com/pnpm/pnpm/issues/15230).
