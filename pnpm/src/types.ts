import { type Config } from '@pnpm/config'
import {
  type LogBase,
  type ReadPackageHook,
} from '@pnpm/types'

export type PnpmOptions = Omit<Config, 'reporter' | 'pnpmfile'> & {
  argv: {
    cooked: string[]
    original: string[]
    remain: string[]
  }
  cliOptions: object
  reporter?: (logObj: LogBase) => void
  pnpmfile: string[]
  packageManager?: {
    name: string
    version: string
  }

  hooks?: {
    readPackage?: ReadPackageHook[]
  }

  ignoreFile?: (filename: string) => boolean
}
