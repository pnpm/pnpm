---
"@pnpm/plugin-commands-patching": patch
"pnpm": patch
---

Fixed `pnpm patch-commit` failing with "unable to access '/.config/git/attributes': Permission denied" error in environments where HOME is unset or non-standard (Docker containers, CI systems).

The issue occurred because pnpm was setting `HOME: ''` and `USERPROFILE: ''` to suppress user git configuration when running `git diff`. This caused git to resolve the home directory (`~`) as root (`/`), leading to permission errors when attempting to access `/.config/git/attributes`.

Now uses `GIT_CONFIG_GLOBAL: os.devNull` instead, which is git's proper mechanism for bypassing user-level configuration without corrupting the home directory path resolution.

Fixes #6537