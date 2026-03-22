---
"@pnpm/store.cafs": patch
"pnpm": patch
---

When pnpm installs a `file:` or `git:` dependency, it now validates that symlinks point within the package directory. Symlinks to paths outside the package root are skipped to prevent local data from being leaked into `node_modules`.

This fixes a security issue where a malicious package could create symlinks to sensitive files (e.g., `/etc/passwd`, `~/.ssh/id_rsa`) and have their contents copied when the package is installed.

Note: This only affects `file:` and `git:` dependencies. Registry packages (npm) have symlinks stripped during publish and are not affected.
