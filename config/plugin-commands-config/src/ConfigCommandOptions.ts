import { Config } from '@pnpm/config'

export type ConfigCommandOptions = Pick<Config,
| 'configDir'
| 'dir'
| 'global'
| 'rawConfig'
> & {
  json?: boolean
  location?: 'global' | 'project'
}
