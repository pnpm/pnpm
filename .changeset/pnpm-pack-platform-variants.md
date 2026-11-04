---
"pacquet": minor
---

`pnpm pack` packs one archive per entry of `publishConfig.variants`, each platform variant as `<name>-<version>~<tag>.tgz`. `pnpm pack --variant <target>` packs only the archive whose entry lists that target.
