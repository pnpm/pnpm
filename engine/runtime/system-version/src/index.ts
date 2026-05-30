import { detectIfCurrentPkgIsExecutable } from '@pnpm/cli.meta'
import type { RuntimeName } from '@pnpm/types'
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

export function getSystemDenoVersionNonCached (): string | undefined {
  try {
    // `deno --version` prints e.g. "deno 1.40.0 (release, ...)\nv8 ..."
    const output = execa.sync('deno', ['--version']).stdout?.toString() ?? ''
    const match = /^deno\s+(\d+\.\d+\.\d\S*)/m.exec(output)
    return match?.[1] ? `v${match[1]}` : undefined
  } catch {
    return undefined
  }
}

export function getSystemBunVersionNonCached (): string | undefined {
  try {
    // `bun --version` prints just the bare version, e.g. "1.1.0".
    const output = execa.sync('bun', ['--version']).stdout?.toString().trim() ?? ''
    return /^\d+\.\d+\.\d+/.test(output) ? `v${output}` : undefined
  } catch {
    return undefined
  }
}

export const getSystemNodeVersion = mem(getSystemNodeVersionNonCached)
export const getSystemDenoVersion = mem(getSystemDenoVersionNonCached)
export const getSystemBunVersion = mem(getSystemBunVersionNonCached)

export function getSystemRuntimeVersion (name: RuntimeName): string | undefined {
  switch (name) {
    case 'node': return getSystemNodeVersion()
    case 'deno': return getSystemDenoVersion()
    case 'bun': return getSystemBunVersion()
  }
}

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
 *    through to the hash — see `findRuntimeNodeVersion` /
 *    `readSnapshotRuntimePin` in `@pnpm/deps.path` for the helpers
 *    that extract the value from a lockfile or graph node.
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
