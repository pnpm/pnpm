---
"pacquet": minor
---

`pnpm publish --upload-variant <target>` uploads the archive of one `publishConfig.variants` entry to a pnpr registry's staging area and prints `<target>=<integrity>`. `pnpm publish --variant-integrity <target>=<integrity>` publishes the version from archives other jobs uploaded, so each job of a CI matrix can upload the archive it built.
