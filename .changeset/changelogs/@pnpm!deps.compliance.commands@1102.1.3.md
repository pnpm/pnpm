## 1102.1.3

### Patch Changes

- `pnpm audit --interactive --fix=update` no longer opens a second prompt for selecting dependencies to update [#14927](https://github.com/pnpm/pnpm/issues/14927).

- `pnpm sbom` now publishes a valid URL in the CycloneDX `externalReferences[].url` and the SPDX `homepage`. An npm shorthand such as `vercel/ms` or `gitlab:group/subgroup/project` is expanded to the `git+https` URL npm derives for it. An scp-style remote such as `git@github.com:vercel/ms.git` is expanded the same way. Any other URL is published in its normalized form, without embedded credentials. A value that names no repository, an email address for example, is left out. pnpm used to publish the raw value, so a shorthand produced a URL that consumers such as Dependency-Track reject [pnpm/pnpm#14773](https://github.com/pnpm/pnpm/issues/14773).

- Updated dependencies:
  - @pnpm/cli.utils@1101.0.28
  - @pnpm/config.reader@1102.2.1
  - @pnpm/config.writer@1100.0.27
  - @pnpm/deps.compliance.audit@1101.0.37
  - @pnpm/deps.compliance.license-scanner@1101.0.5
  - @pnpm/deps.compliance.sbom@1101.0.4
  - @pnpm/deps.security.signatures@1102.0.4
  - @pnpm/installing.commands@1101.3.1
  - @pnpm/lockfile.fs@1100.2.8
  - @pnpm/lockfile.types@1100.1.2
  - @pnpm/lockfile.utils@1102.1.3
  - @pnpm/lockfile.walker@1100.0.24
  - @pnpm/network.fetch@1100.1.17
  - @pnpm/pkg-manifest.utils@1100.4.5
  - @pnpm/text.sanitize@1100.0.1
  - @pnpm/workspace.project-manifest-reader@1100.0.29
