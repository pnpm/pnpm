import { LogBase } from '@pnpm/logger'

export type ReporterFunction = (logObj: LogBase) => void
