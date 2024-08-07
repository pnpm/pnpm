import bole from 'bole'
import { type PnpmError } from '@pnpm/error'

bole.setFastTime()

export const logger = bole('pnpm') as Logger<object, PnpmError>

export interface Logger<T, E extends Error = Error> {
  <Y>(name: string): Logger<Y>
  debug: (log?: T) => void
  info: (log: { message: string, prefix: string }) => void
  warn: (log: { message: string, prefix: string, error?: E }) => void
  error: (err: E, log?: string | E) => void
}

const globalLogger = bole('pnpm:global')

export function globalWarn (message: string): void {
  globalLogger.warn(message)
}

export function globalInfo (message: string): void {
  globalLogger.info(message)
}
