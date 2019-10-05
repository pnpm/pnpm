import baseLogger, {
  LogBase,
} from '@pnpm/logger'
import { ImporterManifest } from '@pnpm/types'

export const packageManifestLogger = baseLogger<PackageManifestMessage>('package-manifest')

export type PackageManifestMessage = {
  prefix: string,
} & ({
  initial: ImporterManifest,
} | {
  updated: ImporterManifest,
})

export type PackageManifestLog = { name: 'pnpm:package-manifest' } & LogBase & PackageManifestMessage
