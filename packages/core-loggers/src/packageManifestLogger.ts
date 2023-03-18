import {
  type LogBase,
  logger,
} from '@pnpm/logger'
import { type ProjectManifest } from '@pnpm/types'

export const packageManifestLogger = logger<PackageManifestMessage>('package-manifest')

export type PackageManifestMessage = {
  prefix: string
} & ({
  initial: ProjectManifest
} | {
  updated: ProjectManifest
})

export type PackageManifestLog = { name: 'pnpm:package-manifest' } & LogBase & PackageManifestMessage
