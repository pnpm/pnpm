import { type Config } from '@pnpm/config'

export type ConfigCommandOptions = Pick<Config,
| 'configDir'
| 'cliOptions'
| 'dir'
| 'global'
| 'npmPath'
| 'rawConfig'
| 'workspaceDir'
> & {
  json?: boolean
  location?: 'global' | 'project'
}
