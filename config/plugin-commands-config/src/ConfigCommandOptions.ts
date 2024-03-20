import type { Config } from '@pnpm/types'

export type ConfigCommandOptions = Pick<
  Config,
  'configDir' | 'cliOptions' | 'dir' | 'global' | 'npmPath' | 'rawConfig'
> & {
  json?: boolean
  location?: 'global' | 'project'
}
