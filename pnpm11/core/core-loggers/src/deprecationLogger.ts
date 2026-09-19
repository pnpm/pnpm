import {
  type LogBase,
  type Logger,
  logger,
} from '@pnpm/logger'

export const deprecationLogger = logger('deprecation') as Logger<DeprecationMessage>

/**
 * A package version the registry reports as deprecated.
 *
 * The deprecation notice itself is deliberately absent. It is free-form text
 * that any publisher can rewrite on an already-published version, so it
 * reaches neither the terminal nor an NDJSON consumer. `pnpm view` shows it
 * on request instead.
 */
export interface DeprecationMessage {
  pkgName: string
  pkgVersion: string
  pkgId: string
  prefix: string
  depth: number
}

export type DeprecationLog = { name: 'pnpm:deprecation' } & LogBase & DeprecationMessage
