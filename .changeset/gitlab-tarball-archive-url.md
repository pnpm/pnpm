---
"@pnpm/resolving.git-resolver": patch
"pnpm": patch
---

Fixed installation of GitLab-hosted dependencies. pnpm now downloads the tarball from `https://gitlab.com/<user>/<project>/-/archive/<sha>/<project>-<sha>.tar.gz` instead of the GitLab API endpoint that contained an encoded slash (`%2F`) between user and project. The encoded slash both triggered `406 Not Acceptable` responses from GitLab and produced virtual store directory names that Node refused to import (`ERR_INVALID_MODULE_SPECIFIER`) [#11533](https://github.com/pnpm/pnpm/issues/11533).
