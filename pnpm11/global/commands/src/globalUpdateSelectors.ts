import { parseNodeSpecifier } from '@pnpm/engine.runtime.node-resolver'
import { calcVersionRange } from '@pnpm/pkg-manifest.utils'
import { parseWantedDependency } from '@pnpm/resolving.parse-wanted-dependency'
import type { RangeSpecStyle } from '@pnpm/types'
import semver from 'semver'

const RUNTIME_ALIASES = new Set(['node', 'deno', 'bun'])

/**
 * The selectors that reinstall a group. With `--latest`, plain version specs,
 * `npm:` aliases, and `runtime:` dependencies target the latest release; `pins`
 * holds back aliases that would otherwise move backwards.
 */
export function depSpecsForUpdate (
  dependencies: Record<string, string>,
  latest?: boolean,
  pins: ReadonlyMap<string, string> = new Map()
): string[] {
  return Object.entries(dependencies).map(([alias, spec]) => {
    const pin = pins.get(alias)
    if (pin != null) return pinnedDepSpec(alias, spec, pin)
    if (latest) {
      const latestSpec = latestDepSpec(alias, spec)
      if (latestSpec != null) return latestSpec
    }
    return `${alias}@${spec}`
  })
}

export function latestDepSpec (alias: string, spec: string): string | undefined {
  if (!spec.includes(':')) return alias
  if (spec.startsWith('npm:')) {
    return latestNpmAliasSpec(alias, spec.slice('npm:'.length))
  }
  if (spec.startsWith('runtime:') || spec.startsWith('tool:')) {
    return latestRuntimeSpec(alias, spec.slice(spec.indexOf(':') + 1))
  }
  return undefined
}

export function applyRuntimeRangeBumps (
  dependencies: Record<string, string>,
  depSpecs: string[],
  pins: ReadonlyMap<string, string>,
  resolvedVersions: Record<string, string>,
  rangeSpecStyle: RangeSpecStyle
): string[] {
  return Object.entries(dependencies).map(([alias, spec], index) => {
    if (pins.has(alias)) return depSpecs[index]
    return bumpedRuntimeLatestSpec(alias, spec, resolvedVersions[alias], rangeSpecStyle) ?? depSpecs[index]
  })
}

function parseNodeRuntimeChannel (runtimeSpec: string): string | undefined {
  try {
    return parseNodeSpecifier(runtimeSpec).releaseChannel
  } catch {
    return undefined
  }
}

function latestNpmAliasSpec (alias: string, npmTarget: string): string | undefined {
  const parsed = parseWantedDependency(npmTarget)
  if (parsed.alias != null) {
    return parsed.bareSpecifier == null || !parsed.bareSpecifier.includes(':')
      ? `${alias}@npm:${parsed.alias}@latest`
      : undefined
  }
  if (parsed.bareSpecifier != null && parsed.bareSpecifier !== '' && !parsed.bareSpecifier.includes(':')) {
    return `${alias}@npm:${alias}@latest`
  }
  return undefined
}

function latestRuntimeSpec (alias: string, runtimeSpec: string): string | undefined {
  if (!RUNTIME_ALIASES.has(alias)) return undefined
  if (alias !== 'node') return `${alias}@runtime:latest`
  const channel = parseNodeRuntimeChannel(runtimeSpec)
  if (channel == null) return undefined
  return channel === 'release' ? 'node@runtime:latest' : `node@runtime:${channel}/latest`
}

function bumpedRuntimeLatestSpec (
  alias: string,
  spec: string,
  resolvedVersion: string | undefined,
  rangeSpecStyle: RangeSpecStyle
): string | undefined {
  if (alias === 'node' || !RUNTIME_ALIASES.has(alias) || resolvedVersion == null || semver.valid(resolvedVersion) == null) {
    return undefined
  }
  if (!spec.startsWith('runtime:') && !spec.startsWith('tool:')) return undefined
  const declaredRange = spec.slice(spec.indexOf(':') + 1).trim()
  if (declaredRange === '' || semver.validRange(declaredRange) == null) return undefined
  const range = calcVersionRange(resolvedVersion, {
    prevSpecifier: declaredRange,
    defaultRangeSpecStyle: rangeSpecStyle,
    isUpdate: true,
  })
  return `${alias}@runtime:${range}`
}

function pinnedDepSpec (alias: string, spec: string, pin: string): string {
  if (!spec.includes(':')) return `${alias}@${pin}`
  if (spec.startsWith('npm:')) {
    const parsed = parseWantedDependency(spec.slice('npm:'.length))
    return `${alias}@npm:${parsed.alias ?? alias}@${pin}`
  }
  if (spec.startsWith('runtime:') || spec.startsWith('tool:')) {
    return pinnedRuntimeSpec(alias, spec.slice(spec.indexOf(':') + 1), pin)
  }
  return `${alias}@${pin}`
}

function pinnedRuntimeSpec (alias: string, runtimeSpec: string, pin: string): string {
  if (alias === 'node') {
    const channel = parseNodeRuntimeChannel(runtimeSpec)
    if (channel != null && channel !== 'release' && parseNodeRuntimeChannel(pin) === 'release') {
      return `node@runtime:${channel}/${pin}`
    }
  }
  return `${alias}@runtime:${pin}`
}
