# @pnpm/os.env.path-extender-windows

> A module for adding a new directory to the Path environment variable of the Windows Registry

## Usage

This adds a `PNPM_HOME` environment variable with the specified directory. And prepends the value of `PNPM_HOME` to the `Path`:

```ts
import { addDirToWindowsEnvPath } from '@pnpm/os.env.path-extender-windows'

await addDirToWindowsEnvPath('C:\\pnpm', {
  // This is optional as 'start' is the default value.
  // You may also use 'end'
  position: 'start',
  // This is the name of the additional env variable that holds the directory.
  // If not set, no additional env variable is used.
  proxyVarName: 'PNPM_HOME',
  // If PNPM_HOME already exist, overwrite it
  overwriteProxyVar: true,
})
//> PNPM_HOME=C:\pnpm
//  Path=%PNPM_HOME%;C:\foo;C:\bar
```

This prepends `C:\pnpm` to the `Path` (no additional environment variable is created:

```ts
import { addDirToWindowsEnvPath } from '@pnpm/os.env.path-extender-windows'

await addDirToWindowsEnvPath('C:\\pnpm')
//> Path=C:\pnpm;C:\foo;C:\bar
```

## License

MIT
