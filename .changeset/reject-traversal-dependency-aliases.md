---
"@pnpm/installing.deps-resolver": patch
"@pnpm/fs.symlink-dependency": patch
"pnpm": patch
---

Reject dependency aliases that contain path-traversal segments (such as `@x/../../../../../.git/hooks`) when reading them from a package manifest or symlinking them into `node_modules`. A malicious registry package could otherwise use a transitive dependency key to make `pnpm install` create symlinks at attacker-chosen paths outside the intended `node_modules` directory.
