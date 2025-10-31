---
"@pnpm/read-project-manifest": minor
"@pnpm/resolve-dependencies": minor
"@pnpm/link-bins": minor
"@pnpm/types": minor
"pnpm": minor
---

**Node.js Runtime Installation for Dependencies.** Added support for automatic Node.js runtime installation for dependencies. pnpm will now install the Node.js version required by a dependency if that dependency declares a Node.js runtime in the "engines" field. For example:

```json
{
  "engines": {
    "runtime": {
      "name": "node",
      "version": "^24.11.0",
      "onFail": "download"
    }
  }
}
```

If the package with the Node.js runtime dependency is a CLI app, pnpm will bind the CLI app to the required Node.js version. This ensures that, regardless of the globally installed Node.js instance, the CLI will use the compatible version of Node.js.

If the package has a `postinstall` script, that script will be executed using the specified Node.js version.

Related PR: [#10141](https://github.com/pnpm/pnpm/pull/10141)
