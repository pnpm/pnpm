---
"@pnpm/installing.commands": minor
"pacquet": minor
"pnpm": minor
---

Added a `--changeset` flag to `pnpm update`. After the update completes, pnpm writes a `.changeset/pnpm-update-<suffix>.md` file declaring a patch bump for every workspace package whose `dependencies` or `optionalDependencies` were changed by the update, including packages that consume an updated catalog entry via the `catalog:` protocol. Private packages, packages without a name, and packages listed in the `ignore` array of `.changeset/config.json` are skipped. If `.changeset/config.json` does not exist, a warning is printed and no changeset is generated.
