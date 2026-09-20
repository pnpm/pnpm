## 1101.0.4

### Patch Changes

- `pnpm sbom` now publishes a valid URL in the CycloneDX `externalReferences[].url` and the SPDX `homepage`. An npm shorthand such as `vercel/ms` or `gitlab:group/subgroup/project` is expanded to the `git+https` URL npm derives for it. An scp-style remote such as `git@github.com:vercel/ms.git` is expanded the same way. Any other URL is published in its normalized form, without embedded credentials. A value that names no repository, an email address for example, is left out. pnpm used to publish the raw value, so a shorthand produced a URL that consumers such as Dependency-Track reject [pnpm/pnpm#14773](https://github.com/pnpm/pnpm/issues/14773).

- `pnpm sbom` now emits a license value as a CycloneDX expression only when it is a valid SPDX license expression. Anything else is emitted as a CycloneDX license name [pnpm/pnpm#14786](https://github.com/pnpm/pnpm/issues/14786).

- Updated dependencies:
  - @pnpm/config.package-is-installable@1100.1.7
  - @pnpm/lockfile.detect-dep-types@1100.0.24
  - @pnpm/lockfile.types@1100.1.2
  - @pnpm/lockfile.utils@1102.1.3
  - @pnpm/lockfile.walker@1100.0.24
  - @pnpm/resolving.resolver-base@1101.3.0
  - @pnpm/store.pkg-finder@1100.0.34
