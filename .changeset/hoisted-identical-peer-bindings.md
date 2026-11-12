---
"pacquet": patch
---

Fixed `pnpm install` creating duplicate copies of a package with identical resolved peers when `nodeLinker` is `hoisted` and `autoInstallPeers` is `false`. This happened when an ancestor depended on a different version of one of those peers [#18003](https://github.com/pnpm/pnpm/issues/18003).
