import {
  LogBase,
  logger,
} from '@pnpm/logger'

export const packageImportMethodLogger = logger('package-import-method')

export interface PackageImportMethodMessage {
  method: 'clone' | 'hardlink' | 'copy'
}

export type PackageImportMethodLog = { name: 'pnpm:package-import-method' } & LogBase & PackageImportMethodMessage
