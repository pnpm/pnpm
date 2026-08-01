---
"@pnpm/building.policy": patch
"pnpm": patch
"pacquet": patch
---

`allowBuilds` entries can now approve git-hosted packages that pnpm downloads as a tarball, such as `github:` dependencies (which are fetched from `codeload.github.com` rather than cloned), by their repository URL without the resolved commit hash. This matches the hashless `git+` matching already supported for cloned git dependencies. For example:

```yaml
allowBuilds:
  "foo@git+https://github.com/org/foo.git": true
```

This approves the package whether pnpm clones it or downloads a tarball, so the entry no longer has to be updated every time the pinned commit changes. GitLab and Bitbucket tarball downloads are matched the same way. Approving or denying a specific resolved commit by its full tarball dep path continues to work.
