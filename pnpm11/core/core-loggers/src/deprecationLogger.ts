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
  /**
   * A version of the same package that is not deprecated, when the resolver
   * knew one. Absent for a resolution reused from the lockfile, which holds
   * no packument to work it out from.
   */
  nonDeprecatedAlternative?: {
    version: string
    /**
     * Whether reaching it means widening the declared range. Only ever true
     * for a dependency that declared a range: a tag says nothing about which
     * versions are acceptable.
     */
    outsideDeclaredRange: boolean
  }
}

export type DeprecationLog = { name: 'pnpm:deprecation' } & LogBase & DeprecationMessage
