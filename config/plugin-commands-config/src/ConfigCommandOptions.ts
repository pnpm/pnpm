import { type Config } from '@pnpm/config'

export type ConfigCommandOptions = Pick<Config,
| 'configDir'
| 'cliOptions'
| 'dir'
| 'global'
| 'npmPath'
| 'rawConfig'
> & {
  json?: boolean
  location?: 'global' | 'project'
}
