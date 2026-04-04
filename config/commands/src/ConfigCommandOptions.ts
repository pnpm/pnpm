import type { Config } from '@pnpm/config.reader'

export type ConfigCommandOptions = Pick<Config,
| 'configDir'
| 'cliOptions'
| 'dir'
| 'global'
| 'authConfig'
| 'workspaceDir'
> & {
  json?: boolean
  location?: 'global' | 'project'
  // The config commands receive the full Config object at runtime
  // and read arbitrary typed properties for display.
  [key: string]: unknown
}
