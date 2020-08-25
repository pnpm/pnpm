import { Config } from '@pnpm/config'
import {
  LogBase,
  ReadPackageHook,
} from '@pnpm/types'

export type PnpmOptions = Omit<Config, 'reporter'> & {
  argv: {
    cooked: string[]
    original: string[]
    remain: string[]
  }
  cliOptions: object
  reporter?: (logObj: LogBase) => void
  packageManager?: {
    name: string
    version: string
  }

  hooks?: {
    readPackage?: ReadPackageHook
  }

  ignoreFile?: (filename: string) => boolean
}
