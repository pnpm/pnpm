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
  config: Config
  context: ConfigContext
  json?: boolean
  location?: 'global' | 'project'
}
