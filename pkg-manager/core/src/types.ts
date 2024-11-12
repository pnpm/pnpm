import { type LogBase } from '@pnpm/logger'
import { type PackagesList } from '@pnpm/modules-yaml'

export type CreatePackagesList = (lastValidatedTimestamp: number) => PackagesList

export type ReporterFunction = (logObj: LogBase) => void
