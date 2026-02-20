import { type Config } from '@pnpm/config'

export type ConfigCommandOptions = Pick<Config,
| 'configDir'
| 'cliOptions'
| 'dir'
| 'global'
| 'rawConfig'
| 'workspaceDir'
> & {
  json?: boolean
  location?: 'global' | 'project'
}
