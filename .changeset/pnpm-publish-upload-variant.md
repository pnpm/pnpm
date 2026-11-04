---
"pacquet": minor
---

`pnpm publish --upload-variant <target>` uploads the archive of one `publishConfig.variants` entry to the staging area of a pnpr registry. It prints `<target>=<integrity>`. `pnpm publish --variant-integrity <target>=<integrity>` publishes the version from archives that other jobs uploaded. With `--recursive`, each value names its package as `<name>~<target>=<integrity>`.
