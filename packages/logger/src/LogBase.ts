import { type LogLevel } from './LogLevel'

export interface OptionalErrorProperties {
  pkgsStack?: Array<{ id: string, name: string, version: string }>
  hint?: string
  package?: { name?: string, pref?: string, version?: string }
  err?: NodeJS.ErrnoException
}

export interface LogBaseTemplate extends OptionalErrorProperties {
  level?: LogLevel
  prefix?: string
  message?: string
}

export interface LogBaseDebug extends LogBaseTemplate {
  level: 'debug'
}

export interface LogBaseError extends LogBaseTemplate {
  level: 'error'
}

export interface LogBaseInfo extends LogBaseTemplate {
  level: 'info'
  prefix: string
  message: string
}

export interface LogBaseWarn extends LogBaseTemplate {
  level: 'warn'
  prefix: string
  message: string
}

export type LogBase = LogBaseDebug | LogBaseError | LogBaseInfo | LogBaseWarn
