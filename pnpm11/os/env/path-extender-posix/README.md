# @pnpm/os.env.path-extender-posix

> A module for adding a new directory to the PATH environment variable on Linux and macOS

This module detects the active shell and updates its config file to extend the value of the PATH environment variable. Supported shells are: bash, zsh, and fish.

## Usage

This adds a `PNPM_HOME` environment variable with the specified directory. And prepends the value of `PNPM_HOME` to the `PATH`:

```ts
import { addDirToPosixEnvPath } from '@pnpm/os.env.path-extender-posix'

await addDirToPosixEnvPath('/home/user/.local/share/pnpm', {
  // This is optional as 'start' is the default value.
  // You may also use 'end'
  position: 'start',
  // This is the name of the additional env variable that holds the directory.
  // If not set, no additional env variable is used.
  proxyVarName: 'PNPM_HOME',
  // If PNPM_HOME already exist, overwrite it
  overwrite: true,
  configSectionName: 'pnpm',
})
//> # pnpm
//  PNPM_HOME=/home/user/.local/share/pnpm
//  Path=$PNPM_HOME:/foo:/bar
//  # pnpm end
```

This prepends `/home/user/.local/share/pnpm` to the `PATH` (no additional environment variable is created:

```ts
import { addDirToPosixEnvPath } from '@pnpm/os.env.path-extender-posix'

await addDirToPosixEnvPath('/home/user/.local/share/pnpm', {
  configSectionName: 'pnpm',
})
//> # pnpm
//  Path=/home/user/.local/share/pnpm:/foo:/bar
//  # pnpm end
```

## License

MIT
