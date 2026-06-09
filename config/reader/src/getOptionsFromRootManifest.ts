import path from 'node:path'

import { envReplace } from '@pnpm/config.env-replace'
import { PnpmError } from '@pnpm/error'
import { globalWarn } from '@pnpm/logger'
import type {
  AllowedDeprecatedVersions,
  PackageExtension,
  PeerDependencyRules,
  PnpmSettings,
  ProjectManifest,
  SupportedArchitectures,
} from '@pnpm/types'
import { map as mapValues } from 'ramda'

export type OptionsFromRootManifest = {
  allowedDeprecatedVersions?: AllowedDeprecatedVersions
  allowUnusedPatches?: boolean
  overrides?: Record<string, string>
  packageExtensions?: Record<string, PackageExtension>
  ignoredOptionalDependencies?: string[]
  patchedDependencies?: Record<string, string>
  peerDependencyRules?: PeerDependencyRules
  supportedArchitectures?: SupportedArchitectures
  allowBuilds?: Record<string, boolean | string>
  requiredScripts?: string[]
} & Pick<PnpmSettings, 'configDependencies' | 'auditConfig' | 'pnprServer' | 'updateConfig'>

interface GetOptionsFromPnpmSettingsOptions {
  manifest?: ProjectManifest
  expandRequestDestinationEnv?: boolean
}

interface ReplaceEnvInSettingsOptions {
  expandRequestDestinationEnv: boolean
}

const REQUEST_DESTINATION_SCALAR_KEYS = new Set(['pnprServer', 'registry'])

export function getOptionsFromPnpmSettings (
  manifestDir: string | undefined,
  pnpmSettings: PnpmSettings,
  manifestOrOpts?: ProjectManifest | GetOptionsFromPnpmSettingsOptions
): OptionsFromRootManifest {
  const opts = isGetOptionsFromPnpmSettingsOptions(manifestOrOpts)
    ? manifestOrOpts
    : manifestOrOpts == null ? {} : { manifest: manifestOrOpts }
  const settings: OptionsFromRootManifest = replaceEnvInSettings(pnpmSettings, {
    expandRequestDestinationEnv: opts.expandRequestDestinationEnv ?? false,
  })
  if (settings.overrides) {
    assertValidOverrides(settings.overrides)
    if (Object.keys(settings.overrides).length === 0) {
      delete settings.overrides
    } else {
      warnAboutDeprecatedVersionReferences(settings.overrides)
      if (opts.manifest) {
        settings.overrides = mapValues(createVersionReferencesReplacer(opts.manifest), settings.overrides)
      }
    }
  }
  if (pnpmSettings.patchedDependencies) {
    settings.patchedDependencies = { ...pnpmSettings.patchedDependencies }
    for (const [dep, patchFile] of Object.entries(pnpmSettings.patchedDependencies)) {
      if (manifestDir == null || path.isAbsolute(patchFile)) continue
      settings.patchedDependencies[dep] = path.join(manifestDir, patchFile)
    }
  }

  return settings
}

function isGetOptionsFromPnpmSettingsOptions (
  value: ProjectManifest | GetOptionsFromPnpmSettingsOptions | undefined
): value is GetOptionsFromPnpmSettingsOptions {
  return value != null && ('expandRequestDestinationEnv' in value || 'manifest' in value)
}

function assertValidOverrides (overrides: unknown): asserts overrides is Record<string, string> {
  if (overrides == null || typeof overrides !== 'object' || Array.isArray(overrides)) {
    throw new PnpmError('INVALID_OVERRIDES', `The overrides field should be an object, but got ${renderReceivedType(overrides)}`)
  }
  for (const [selector, spec] of Object.entries(overrides)) {
    if (typeof spec !== 'string') {
      throw new PnpmError('INVALID_OVERRIDES', `The value of overrides.${selector} should be a string, but got ${renderReceivedType(spec)}`)
    }
  }
}

function renderReceivedType (value: unknown): string {
  if (value === null) return 'null'
  if (Array.isArray(value)) return 'array'
  return typeof value
}

function replaceEnvInSettings (
  settings: PnpmSettings,
  opts: ReplaceEnvInSettingsOptions
): PnpmSettings {
  const newSettings: PnpmSettings = {}
  for (const [key, value] of Object.entries(settings)) {
    const newKey = envReplace(key, process.env)
    if (typeof value === 'string') {
      if (REQUEST_DESTINATION_SCALAR_KEYS.has(newKey) && !opts.expandRequestDestinationEnv && hasEnvPlaceholder(value)) continue
      // @ts-expect-error
      newSettings[newKey as keyof PnpmSettings] = envReplace(value, process.env)
    } else if (newKey === 'registries' || newKey === 'namedRegistries') {
      newSettings[newKey as keyof PnpmSettings] = (opts.expandRequestDestinationEnv
        ? replaceEnvInStringValues(value)
        : copyStringValuesWithoutEnvPlaceholders(value)) as never
    } else {
      newSettings[newKey as keyof PnpmSettings] = value
    }
  }
  return newSettings
}

function replaceEnvInStringValues (value: unknown): unknown {
  if (value == null || typeof value !== 'object' || Array.isArray(value)) return value
  const out: Record<string, unknown> = {}
  for (const [k, v] of Object.entries(value as Record<string, unknown>)) {
    out[k] = typeof v === 'string' ? envReplace(v, process.env) : v
  }
  return out
}

function copyStringValuesWithoutEnvPlaceholders (value: unknown): unknown {
  if (value == null || typeof value !== 'object' || Array.isArray(value)) return value
  const out: Record<string, unknown> = {}
  for (const [k, v] of Object.entries(value as Record<string, unknown>)) {
    if (typeof v === 'string' && hasEnvPlaceholder(v)) continue
    out[k] = v
  }
  return out
}

function hasEnvPlaceholder (value: string): boolean {
  return /\$\{[^}]+\}/.test(value)
}

function warnAboutDeprecatedVersionReferences (overrides: Record<string, string>): void {
  const selectors = Object.keys(overrides).filter((selector) => overrides[selector][0] === '$')
  if (selectors.length === 0) return
  globalWarn(
    `The "$" version reference syntax in overrides is deprecated (used by: ${selectors.join(', ')}). ` +
    'Define the version in a catalog and reference it with the "catalog:" protocol instead. ' +
    'See https://pnpm.io/catalogs'
  )
}

function createVersionReferencesReplacer (manifest: ProjectManifest): (spec: string) => string {
  const allDeps = {
    ...manifest.devDependencies,
    ...manifest.dependencies,
    ...manifest.optionalDependencies,
  }
  return replaceVersionReferences.bind(null, allDeps)
}

function replaceVersionReferences (dep: Record<string, string>, spec: string): string {
  if (!(spec[0] === '$')) return spec
  const dependencyName = spec.slice(1)
  const newSpec = dep[dependencyName]
  if (newSpec) return newSpec
  throw new PnpmError(
    'CANNOT_RESOLVE_OVERRIDE_VERSION',
    `Cannot resolve version ${spec} in overrides. The direct dependencies don't have dependency "${dependencyName}".`
  )
}
