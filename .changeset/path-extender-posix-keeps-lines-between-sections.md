---
"@pnpm/os.env.path-extender-posix": patch
"@pnpm/os.env.path-extender": patch
---

`addDirToPosixEnvPath` no longer deletes the lines between two `# pnpm` sections of a shell config file when it overwrites the pnpm section [#12282](https://github.com/pnpm/pnpm/issues/12282).
