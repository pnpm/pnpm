## 1102.3.9

### Patch Changes

- pnpm now recognizes Capawesome Cloud and RWX as CI environments.

- The warning about unrecognized settings now marks `projectScopedOptionalPeers` as a setting of a newer pnpm version.

- A `${VAR}` placeholder in a registry URL in `pnpm-workspace.yaml` or the project `.npmrc` now fails the command. pnpm does not expand environment variables in these files. Before, pnpm ignored such a URL and resolved packages from another registry, such as npmjs.org. pnpm now warns when it ignores a placeholder in a proxy, `pnprServer`, or `userAgent` setting in `pnpm-workspace.yaml` [#17762](https://github.com/pnpm/pnpm/issues/17762).

- A workspace project that downloads its own Node.js through `devEngines.runtime` now has its `engines.node` checked against that Node.js. Before, pnpm checked it against the shared Node.js version, so `engineStrict` could reject a project whose scripts run on a compatible runtime [pnpm/tasks#300](https://github.com/pnpm/tasks/issues/300).

- Updated dependencies:
  - @pnpm/hooks.pnpmfile@1100.0.39
  - @pnpm/pkg-manifest.utils@1100.4.12
  - @pnpm/workspace.project-manifest-reader@1100.1.6
