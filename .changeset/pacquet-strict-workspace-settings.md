---
"pacquet": minor
---

Unrecognized settings in a project's `pnpm-workspace.yaml` are now reported instead of being ignored silently. They warn — suggesting the closest real setting name when the key looks like a typo — and fail the command with `ERR_PNPM_UNRECOGNIZED_WORKSPACE_SETTINGS` when the project pins a pnpm version the running pnpm satisfies, since a satisfied pin means the setting cannot be meant for a different pnpm version. The `pnpm config` subcommands never fail on such keys, so a broken file can still be inspected and repaired, and `pnpm config get <key>` prints the value with no warnings at all. Keys the global config file cannot set are likewise split between workspace-only settings (still directed to `pnpm-workspace.yaml`) and settings unknown to this version.
