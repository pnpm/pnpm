import type { Config } from '@pnpm/config.reader'

export type ConfigCommandOptions = Pick<Config,
| 'configDir'
| 'cliOptions'
| 'dir'
| 'effectiveConfig'
| 'global'
| 'authConfig'
| 'workspaceDir'
> & {
  json?: boolean
  location?: 'global' | 'project'
}
