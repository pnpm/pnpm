---
"pnpm": patch
---

Fixed `pnpm install --offline --frozen-lockfile` failing the release-age check when workspace manifests are copied after `pnpm fetch` [#18409](https://github.com/pnpm/pnpm/issues/18409).
