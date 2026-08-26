---
"pacquet": major
---

A project's `pnpm-workspace.yaml` may no longer carry a setting pnpm does not recognize. Such a setting used to be ignored in silence — a misspelled `minimumReleaseAge` dropped the policy it was meant to set, and nothing said so. Now it is reported, suggesting the closest real setting name when the key looks like a typo, and it fails the command with `ERR_PNPM_UNRECOGNIZED_WORKSPACE_SETTINGS` when the project pins a pnpm version the running pnpm satisfies: with the pin honored, the setting cannot be meant for a different pnpm version, so it is a mistake to fix rather than a key to ignore. Everywhere else it is a warning, so a project that has yet to be cleaned up keeps working.

The `pnpm config` subcommands never fail on such a setting, so a broken file can still be inspected and repaired, and `pnpm config get <key>` prints the value with no warnings at all. Keys the global config file cannot set are likewise split between workspace-only settings (still directed to `pnpm-workspace.yaml`) and settings unknown to this version.
