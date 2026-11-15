import { parseNodeSpecifier } from '@pnpm/engine.runtime.node-resolver'
import { calcVersionRange } from '@pnpm/pkg-manifest.utils'
import { parseWantedDependency } from '@pnpm/resolving.parse-wanted-dependency'
import type { RangeSpecStyle } from '@pnpm/types'
import semver from 'semver'

const RUNTIME_ALIASES = new Set(['node', 'deno', 'bun'])

export interface ApplyRuntimeRangeBumpsOptions {
  dependencies: Record<string, string>
  depSpecs: string[]
  pins: ReadonlyMap<string, string>
  resolvedVersions: Record<string, string>
  rangeSpecStyle: RangeSpecStyle
}

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

export function needsRuntimeRangeBump (
  dependencies: Record<string, string>,
  pins: ReadonlyMap<string, string>
): boolean {
  return Object.entries(dependencies).some(
    ([alias, spec]) => !pins.has(alias) && declaredRuntimeRange(alias, spec) != null
  )
}

export function applyRuntimeRangeBumps (opts: ApplyRuntimeRangeBumpsOptions): string[] {
  const { dependencies, depSpecs, pins, resolvedVersions, rangeSpecStyle } = opts
  return Object.entries(dependencies).map(([alias, spec], index) => {
    if (pins.has(alias)) return depSpecs[index]
    return bumpedRuntimeLatestSpec({
      alias,
      spec,
      resolvedVersion: resolvedVersions[alias],
      rangeSpecStyle,
    }) ?? depSpecs[index]
  })
}

function pinnedDepSpec (alias: string, spec: string, pin: string): string {
  if (!spec.includes(':')) return `${alias}@${pin}`
  if (spec.startsWith('npm:')) {
    const target = parseNpmTarget(alias, spec.slice('npm:'.length))
    return `${alias}@npm:${target?.targetPkg ?? alias}@${pin}`
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

function latestNpmAliasSpec (alias: string, npmBody: string): string | undefined {
  const target = parseNpmTarget(alias, npmBody)
  if (target == null) return undefined
  if (target.versionSpec != null && target.versionSpec.includes(':')) return undefined
  return `${alias}@npm:${target.targetPkg}@latest`
}

function parseNpmTarget (
  alias: string,
  npmBody: string
): { targetPkg: string, versionSpec?: string } | undefined {
  if (npmBody === '') return undefined
  if (semver.validRange(npmBody) != null) {
    return { targetPkg: alias, versionSpec: npmBody }
  }
  const parsed = parseWantedDependency(npmBody)
  if (parsed.alias != null) {
    return { targetPkg: parsed.alias, versionSpec: parsed.bareSpecifier }
  }
  if (parsed.bareSpecifier != null && parsed.bareSpecifier !== '') {
    return { targetPkg: alias, versionSpec: parsed.bareSpecifier }
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

function parseNodeRuntimeChannel (runtimeSpec: string): string | undefined {
  try {
    return parseNodeSpecifier(runtimeSpec).releaseChannel
  } catch {
    return undefined
  }
}

function bumpedRuntimeLatestSpec (opts: {
  alias: string
  spec: string
  resolvedVersion: string | undefined
  rangeSpecStyle: RangeSpecStyle
}): string | undefined {
  const { alias, spec, resolvedVersion, rangeSpecStyle } = opts
  if (resolvedVersion == null || semver.valid(resolvedVersion) == null) return undefined
  const declaredRange = declaredRuntimeRange(alias, spec)
  if (declaredRange == null) return undefined
  const range = calcVersionRange(resolvedVersion, {
    prevSpecifier: declaredRange,
    defaultRangeSpecStyle: rangeSpecStyle,
    isUpdate: true,
  })
  return `${alias}@runtime:${range}`
}

function declaredRuntimeRange (alias: string, spec: string): string | undefined {
  if (alias === 'node' || !RUNTIME_ALIASES.has(alias)) return undefined
  if (!spec.startsWith('runtime:') && !spec.startsWith('tool:')) return undefined
  const declaredRange = spec.slice(spec.indexOf(':') + 1).trim()
  if (declaredRange === '' || semver.validRange(declaredRange) == null) return undefined
  return declaredRange
}
