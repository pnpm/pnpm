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
 * a package's lifecycle scripts — so two installs that materialize the
 * same package on the same host produce the same key.
 *
 * The Node version is resolved in this order:
 *
 * 1. `nodeVersion` argument when provided. Callers use this to thread
 *    a project-pinned runtime (`engines.runtime` / `devEngines.runtime`)
 *    through to the hash — see {@link findRuntimeNodeVersion} for the
 *    helper that extracts the value from a lockfile.
 * 2. {@link getSystemNodeVersion} — the `node` on the user's `PATH`,
 *    or `process.version` when not SEA-bundled.
 * 3. `process.version` as a last-resort fallback when the host has
 *    no `node` on `PATH` (rare: SEA pnpm with no separately-installed
 *    Node). Scripts cannot run in that scenario regardless, so the
 *    cache key is effectively unused — the fallback exists only to
 *    keep the value deterministic.
 *
 * Anchoring to a project-pinned or script-runner Node — not to pnpm's
 * own `process.version` — matters most when pnpm ships via the
 * `@pnpm/exe` SEA bundle, which has an embedded Node distinct from
 * the one that actually runs lifecycle scripts. Without the override,
 * a project with `devEngines.runtime: node@22` would still hash under
 * the SEA-runner's Node major, splitting the cache across two pnpm
 * installations on the same machine even though both run scripts on
 * the same pinned Node.
 */
export function engineName (nodeVersion?: string): string {
  const version = nodeVersion ?? getSystemNodeVersion() ?? process.version
  const stripped = version.startsWith('v') ? version.slice(1) : version
  const major = stripped.split('.')[0]
  return `${process.platform};${process.arch};node${major}`
}

/**
 * Scan an iterable of lockfile snapshot keys for the resolved
 * `engines.runtime` / `devEngines.runtime` Node version and return
 * its bare version string (e.g. `"22.11.0"`), or `undefined` when
 * the project doesn't pin a runtime.
 *
 * Pnpm's runtime resolver writes the pinned Node into the lockfile as
 * a snapshot with key `node@runtime:<version>[(<peers>)]`
 * (see [`engine/runtime/node-resolver/src/index.ts`](https://github.com/pnpm/pnpm/blob/29a42efc3b/engine/runtime/node-resolver/src/index.ts)).
 * The first such key found is treated as authoritative — workspaces
 * with conflicting pins across importers are pathological and the
 * resolver rejects them before they reach the lockfile.
 *
 * Callers typically pass `Object.keys(lockfile.packages ?? {})` — the
 * in-memory `LockfileObject` merges the on-disk `packages:` and
 * `snapshots:` sections under a single `packages` field, so its keys
 * include every snapshot key the install will hash.
 */
export function findRuntimeNodeVersion (snapshotKeys: Iterable<string>): string | undefined {
  const prefix = 'node@runtime:'
  for (const key of snapshotKeys) {
    if (!key.startsWith(prefix)) continue
    // Strip peer-context suffix `(...)` — `node@runtime:22.11.0(node@22.11.0)`
    // resolves to the same Node version as `node@runtime:22.11.0`,
    // so peer-stripped and peer-bearing keys yield the same answer.
    const versionWithPeers = key.slice(prefix.length)
    const parenAt = versionWithPeers.indexOf('(')
    return parenAt === -1 ? versionWithPeers : versionWithPeers.slice(0, parenAt)
  }
  return undefined
}
