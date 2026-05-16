import { detectIfCurrentPkgIsExecutable } from '@pnpm/cli.meta'
import * as execa from 'execa'
import mem from 'memoize'

export function getSystemNodeVersionNonCached (): string | undefined {
  if (detectIfCurrentPkgIsExecutable()) {
    try {
      return execa.sync('node', ['--version']).stdout?.toString()
    } catch {
      // Node.js is not installed on the system
      return undefined
    }
  }
  return process.version
}

export const getSystemNodeVersion = mem(getSystemNodeVersionNonCached)

/**
 * The `<platform>;<arch>;node<major>` string used as the side-effects
 * cache-key prefix and the engine portion of the global-virtual-store
 * hash. Identifies the runtime environment that built (or will build)
 * a package's lifecycle scripts — so two installs that materialise the
 * same package on the same host produce the same key.
 *
 * Anchored to {@link getSystemNodeVersion} rather than `process.version`
 * because pnpm distributed via the `@pnpm/exe` SEA bundle ships its
 * own embedded Node, but spawns lifecycle scripts using the `node` on
 * the user's `PATH`. Using `process.version` would partition the
 * side-effects cache and the GVS by pnpm's *runner* Node (e.g.
 * `node26` for the SEA build), even though scripts run on the
 * different shell `node` (e.g. `node24`). Anchoring to the
 * script-runner Node keeps two pnpm installations on the same
 * machine — one SEA, one npm-package — agreeing on the cache key, and
 * lets pacquet (which detects `node --version` from `PATH`) write to
 * the same slots pnpm reads.
 *
 * Falls back to `process.version` when the host has no `node` on
 * `PATH` (rare: SEA pnpm with no separately-installed Node). In that
 * scenario lifecycle scripts cannot run regardless, so the cache key
 * is effectively unused — the fallback exists only to keep the value
 * deterministic.
 */
export function engineName (): string {
  const version = getSystemNodeVersion() ?? process.version
  const stripped = version.startsWith('v') ? version.slice(1) : version
  const major = stripped.split('.')[0]
  return `${process.platform};${process.arch};node${major}`
}
