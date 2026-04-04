import type { Config, ConfigContext } from '@pnpm/config.reader'

export type ConfigCommandOptions = Pick<Config,
| 'configDir'
| 'dir'
| 'global'
| 'authConfig'
| 'workspaceDir'
> & Pick<ConfigContext,
| 'cliOptions'
> & {
  json?: boolean
  location?: 'global' | 'project'
  // The config commands receive the full Config object at runtime
  // and read arbitrary typed properties for display.
  [key: string]: unknown
}
