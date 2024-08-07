import { LogLevel } from './LogLevel'

export interface LogBaseTemplate {
  level?: LogLevel
  prefix?: string
  message?: string
  pkgsStack?: Array<{ id: string, name: string, version: string }>
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
