---
"pacquet": patch
---

Auto-installed peer dependencies wanted by multiple packages under distinct but compatible ranges now resolve through the ranges' semver intersection (`2` + `^2.2.0` install one provider matching `>=2.2.0 <3.0.0-0`), matching pnpm. Previously such peers were only auto-installed when every consumer declared the identical range or `autoInstallPeersFromHighestMatch` was enabled.
