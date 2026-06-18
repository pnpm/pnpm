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

export type ResolutionsStatus = {
  ignoredResolutions: boolean
  usedResolutions: boolean
}

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
  resolutionsStatus?: ResolutionsStatus
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
  if (settings.overrides != null) assertValidOverrides(settings.overrides)
  const resolutions = opts.manifest?.resolutions
  if (resolutions != null) assertValidOverrides(resolutions, 'resolutions')
  const hasOverrides = settings.overrides != null && Object.keys(settings.overrides).length > 0
  const hasResolutions = resolutions != null && Object.keys(resolutions).length > 0
  if (hasResolutions && !hasOverrides) {
    // Values are copied verbatim — `${VAR}` placeholders are NOT expanded.
    // Unlike `pnpm-workspace.yaml` overrides (which expand env vars through
    // `replaceEnvInSettings`), `package.json` is a repo-controlled manifest
    // and its `resolutions` flow into the lockfile's `overrides`, a shared
    // and persisted artifact. Expanding env vars here would materialize
    // victim environment secrets into the lockfile. Users who need env
    // expansion should move the override to `pnpm-workspace.yaml`.
    settings.overrides = { ...resolutions }
  }
  if (settings.overrides != null) {
    if (Object.keys(settings.overrides).length === 0) {
      delete settings.overrides
    } else {
      warnAboutDeprecatedVersionReferences(settings.overrides)
      if (opts.manifest) {
        settings.overrides = mapValues(createVersionReferencesReplacer(opts.manifest), settings.overrides)
      }
    }
  }
  if (hasResolutions) {
    settings.resolutionsStatus = {
      ignoredResolutions: hasOverrides,
      usedResolutions: !hasOverrides,
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

function assertValidOverrides (overrides: unknown, fieldName: 'overrides' | 'resolutions' = 'overrides'): asserts overrides is Record<string, string> {
  const errorCode = fieldName === 'resolutions' ? 'INVALID_RESOLUTIONS' : 'INVALID_OVERRIDES'
  if (overrides == null || typeof overrides !== 'object' || Array.isArray(overrides)) {
    throw new PnpmError(errorCode, `The ${fieldName} field should be an object, but got ${renderReceivedType(overrides)}`)
  }
  for (const [selector, spec] of Object.entries(overrides)) {
    if (typeof spec !== 'string') {
      throw new PnpmError(errorCode, `The value of ${fieldName}.${sanitizeForLog(selector)} should be a string, but got ${renderReceivedType(spec)}`)
    }
  }
}

// Strip ASCII control characters (incl. `\n`, `\r`, `\t`) from a manifest-
// sourced string before it is interpolated into an error/warning message.
// Repo-controlled values can otherwise inject fake log lines or ANSI escape
// sequences into CI output. Non-string inputs are returned unchanged.
function sanitizeForLog (value: string): string {
  // eslint-disable-next-line no-control-regex
  return value.replace(/[\u0000-\u001F\u007F]/g, '?')
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
  // `${VAR}` is the env-placeholder syntax (preserved literally — env
  // expansion is intentionally not applied to manifest resolutions); it
  // must not trigger the `$dep`-version-reference deprecation warning.
  // `replaceVersionReferences` applies the same `${` -> literal
  // disambiguation when resolving.
  const selectors = Object.keys(overrides).filter((selector) => {
    const spec = overrides[selector]
    return spec[0] === '$' && spec[1] !== '{'
  })
  if (selectors.length === 0) return
  globalWarn(
    `The "$" version reference syntax in overrides is deprecated (used by: ${selectors.map(sanitizeForLog).join(', ')}). ` +
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
  // `${VAR}` is the env-placeholder syntax, not a `$dep` version reference.
  // The two happen to share a leading `$`, but the brace disambiguates: a
  // version reference is always `$ident` (no brace). Env placeholders are
  // preserved literally so they don't materialize victim environment
  // secrets into the lockfile overrides.
  if (spec[0] !== '$' || spec[1] === '{') return spec
  const dependencyName = spec.slice(1)
  const newSpec = dep[dependencyName]
  if (newSpec) return newSpec
  throw new PnpmError(
    'CANNOT_RESOLVE_OVERRIDE_VERSION',
    `Cannot resolve version ${sanitizeForLog(spec)} in overrides. The direct dependencies don't have dependency "${sanitizeForLog(dependencyName)}".`
  )
}
