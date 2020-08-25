import baseLogger, {
  LogBase,
} from '@pnpm/logger'
import { ProjectManifest } from '@pnpm/types'

export const packageManifestLogger = baseLogger<PackageManifestMessage>('package-manifest')

export type PackageManifestMessage = {
  prefix: string
} & ({
  initial: ProjectManifest
} | {
  updated: ProjectManifest
})

export type PackageManifestLog = { name: 'pnpm:package-manifest' } & LogBase & PackageManifestMessage
