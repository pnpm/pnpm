---
"@pnpm/config.reader": minor
"pnpm": minor
---

Settings that no supported pnpm version recognizes get their own warning. A key in the global config file that this version of pnpm does not read is no longer reported with advice to move it to a project-level `pnpm-workspace.yaml` (where it would be ignored too); the warning now says the setting is not recognized by this version of pnpm, names the pnpm version that does read it when there is one (for example, `globalShims` is a pnpm v12 setting), and suggests the closest real setting name when the key looks like a typo. Unrecognized and non-camelCase keys in a project's `pnpm-workspace.yaml`, previously ignored silently, are now reported the same way. `pnpm config get <key>` and `pnpm get <key>` no longer print config-load warnings, so a script capturing the value gets the value alone.
