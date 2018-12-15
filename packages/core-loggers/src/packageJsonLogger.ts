import baseLogger, {
  LogBase,
} from '@pnpm/logger'
import { PackageJson } from '@pnpm/types'

export const packageJsonLogger = baseLogger<PackageJsonMessage>('package-json')

export type PackageJsonMessage = {
  prefix: string,
} & ({
  initial: PackageJson,
} | {
  updated: object,
})

export type PackageJsonLog = {name: 'pnpm:package-json'} & LogBase & PackageJsonMessage
