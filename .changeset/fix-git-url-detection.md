---
"@pnpm/git-resolver": minor
"@pnpm/tarball-resolver": minor
"@pnpm/default-resolver": minor
"pnpm": patch
---

Support plain `http://` and `https://` URLs ending with `.git` as git repository dependencies.

Previously, URLs like `https://gitea.example.org/user/repo.git#commit` were not recognized as git repositories because they lacked the `git+` prefix (e.g., `git+https://`). This caused issues when installing dependencies from self-hosted git servers like Gitea or Forgejo that don't provide tarball downloads.

Changes:
- The git resolver now runs before the tarball resolver, ensuring git URLs are handled by the correct resolver
- The git resolver now recognizes plain `http://` and `https://` URLs ending in `.git` as git repositories
- Removed the `isRepository` check from the tarball resolver since it's no longer needed with the new resolver order

Fixes #10468
