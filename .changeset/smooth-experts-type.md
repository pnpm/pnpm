---
"@pnpm/read-project-manifest": minor
"@pnpm/resolve-dependencies": minor
"@pnpm/package-requester": minor
"@pnpm/resolver-base": minor
"@pnpm/fetcher-base": minor
"@pnpm/pick-fetcher": minor
"@pnpm/headless": minor
"@pnpm/client": minor
"@pnpm/node.resolver": minor
"@pnpm/node.fetcher": minor
"@pnpm/core": minor
"@pnpm/lockfile.types": minor
"@pnpm/lockfile.utils": minor
"pnpm": minor
---

Added support for resolving and downloading the Node.js runtime specified in the [devEngines](https://github.com/openjs-foundation/package-metadata-interoperability-collab-space/issues/15) field of `package.json`.

Usage example:

```json
{
  "devEngines": {
    "runtime": {
      "name": "node",
      "version": "^24.4.0",
      "onFail": "download"
    }
  }
}
```

When running `pnpm install`, pnpm will resolve Node.js to the latest version that satisfies the specified range and install it as a dependency of the project. As a result, when running scripts, the locally installed Node.js version will be used.

Unlike the existing options, `useNodeVersion` and `executionEnv.nodeVersion`, this new field supports version ranges, which are locked to exact versions during installation. The resolved version is stored in the pnpm lockfile, along with an integrity checksum for future validation of the Node.js content's validity.

Related PR: [#9755](https://github.com/pnpm/pnpm/pull/9755).
