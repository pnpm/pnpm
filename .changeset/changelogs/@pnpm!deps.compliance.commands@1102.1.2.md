## 1102.1.2

### Patch Changes

- `pnpm sbom` now omits package author fields when the manifest author name is empty or contains only whitespace [pnpm/pnpm#14685](https://github.com/pnpm/pnpm/issues/14685). In a filtered or split workspace run, only a project with no `author` field inherits the workspace root's author.

- Updated dependencies:
  - @pnpm/cli.utils@1101.0.27
  - @pnpm/config.reader@1102.2.0
  - @pnpm/config.writer@1100.0.26
  - @pnpm/deps.compliance.audit@1101.0.36
  - @pnpm/deps.compliance.license-scanner@1101.0.4
  - @pnpm/deps.compliance.sbom@1101.0.3
  - @pnpm/deps.security.signatures@1102.0.3
  - @pnpm/installing.commands@1101.3.0
  - @pnpm/lockfile.fs@1100.2.7
  - @pnpm/lockfile.utils@1102.1.2
  - @pnpm/lockfile.walker@1100.0.23
  - @pnpm/network.fetch@1100.1.16
  - @pnpm/pkg-manifest.utils@1100.4.4
  - @pnpm/workspace.project-manifest-reader@1100.0.28
