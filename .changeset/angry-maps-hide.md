---
"@pnpm/plugin-commands-publishing": minor
"@pnpm/plugin-commands-script-runners": minor
"@pnpm/filter-workspace-packages": minor
"@pnpm/plugin-commands-licenses": minor
"@pnpm/plugin-commands-patching": minor
"@pnpm/resolve-dependencies": minor
"@pnpm/package-is-installable": minor
"@pnpm/package-requester": minor
"@pnpm/plugin-commands-rebuild": minor
"@pnpm/store-controller-types": minor
"@pnpm/plugin-commands-store": minor
"@pnpm/license-scanner": minor
"@pnpm/filter-lockfile": minor
"@pnpm/workspace.find-packages": minor
"@pnpm/headless": minor
"@pnpm/deps.graph-builder": minor
"@pnpm/core": minor
"@pnpm/types": minor
"@pnpm/cli-utils": minor
"@pnpm/config": minor
"pnpm": minor
---

Support for multiple architectures when installing dependencies [#5965](https://github.com/pnpm/pnpm/issues/5965).

You can now specify architectures for which you'd like to install optional dependencies, even if they don't match the architecture of the system running the install. Use the `supportedArchitectures` field in `package.json` to define your preferences.

For example, the following configuration tells pnpm to install optional dependencies for Windows x64:

```json
{
  "pnpm": {
    "supportedArchitectures": {
      "os": ["win32"],
      "cpu": ["x64"]
    }
  }
}
```

Whereas this configuration will have pnpm install optional dependencies for Windows, macOS, and the architecture of the system currently running the install. It includes artifacts for both x64 and arm64 CPUs:

```json
{
  "pnpm": {
    "supportedArchitectures": {
      "os": ["win32", "darwin", "current"],
      "cpu": ["x64", "arm64"]
    }
  }
}
```

Additionally, `supportedArchitectures` also supports specifying the `libc` of the system.
