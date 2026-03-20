---
"@pnpm/building.commands": minor
"@pnpm/installing.deps-installer": patch
"@pnpm/installing.commands": minor
"pnpm": minor
---

Allow `pnpm approve-builds` to receive positional arguments for approving or denying packages without the interactive prompt. Prefix a package name with `!` to deny it. Only mentioned packages are affected; the rest are left untouched.

During install, packages with ignored builds that are not yet listed in `allowBuilds` are automatically added with a placeholder value. This makes them visible in `pnpm-workspace.yaml` so users can manually change them to `true` or `false` without running `pnpm approve-builds`.
