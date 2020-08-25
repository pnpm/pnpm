import baseLogger, {
  LogBase,
} from '@pnpm/logger'

export const packageImportMethodLogger = baseLogger('package-import-method')

export interface PackageImportMethodMessage {
  method: 'clone' | 'hardlink' | 'copy'
}

export type PackageImportMethodLog = {name: 'pnpm:package-import-method'} & LogBase & PackageImportMethodMessage
