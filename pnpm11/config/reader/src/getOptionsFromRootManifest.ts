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

const REQUEST_DESTINATION_SCALAR_KEYS = new Set(['pnprServer', 'registry', 'httpProxy', 'httpsProxy', 'noProxy', 'proxy', 'noproxy'])

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
  translateUpdateSettings(pnpmSettings, settings)
  translateAuditSettings(pnpmSettings, settings)

  return settings
}

/**
 * Translates the user-facing `update` settings section into the internal
 * `updateConfig` shape that the rest of pnpm reads, and removes the raw
 * `update` key from the returned options.
 *
 * The removal is load-bearing: these options are merged into the global config,
 * where `update` is the boolean flag that turns an install into an update. A
 * leaked `update` object would be truthy and make a plain `pnpm install` behave
 * like `pnpm update`.
 *
 * `updateConfig` is the deprecated spelling, kept working until the next major.
 * When both are set, `update` wins.
 */
function translateUpdateSettings (pnpmSettings: PnpmSettings, settings: OptionsFromRootManifest): void {
  delete (settings as { update?: unknown }).update
  if (pnpmSettings.update == null) return
  if (pnpmSettings.updateConfig != null) {
    globalWarn('Both the "update" and "updateConfig" settings are set. The deprecated "updateConfig" setting is ignored in favor of "update".')
  }
  if (pnpmSettings.update.ignoreDeps == null) {
    settings.updateConfig = {}
    return
  }
  assertStringArray(pnpmSettings.update.ignoreDeps, 'update.ignoreDeps')
  settings.updateConfig = { ignoreDependencies: pnpmSettings.update.ignoreDeps }
}

/**
 * Translates the user-facing `audit` settings section into the internal
 * `auditConfig` / `auditLevel` settings, and removes the raw `audit` key.
 *
 * `auditConfig` and `auditLevel` are the deprecated spellings, kept working
 * until the next major. When the `audit` section provides a value, it wins
 * over its deprecated counterpart (with a warning).
 */
function translateAuditSettings (pnpmSettings: PnpmSettings, settings: OptionsFromRootManifest): void {
  delete (settings as { audit?: unknown }).audit
  const audit = pnpmSettings.audit
  if (audit == null) return
  if (audit.ignore != null) {
    assertStringArray(audit.ignore, 'audit.ignore')
    if (pnpmSettings.auditConfig != null) {
      globalWarn('Both the "audit" and "auditConfig" settings are set. The deprecated "auditConfig" setting is ignored in favor of "audit".')
    }
    settings.auditConfig = { ...settings.auditConfig, ignoreGhsas: audit.ignore }
  }
  if (audit.level != null) {
    if (!AUDIT_LEVELS.has(audit.level)) {
      throw new PnpmError('INVALID_SETTING', `The "audit.level" setting should be one of ${Array.from(AUDIT_LEVELS).join(', ')}, but got ${JSON.stringify(audit.level)}`)
    }
    if ((pnpmSettings as { auditLevel?: unknown }).auditLevel != null) {
      globalWarn('Both the "audit" and "auditLevel" settings are set. The deprecated "auditLevel" setting is ignored in favor of "audit".')
    }
    ;(settings as { auditLevel?: string }).auditLevel = audit.level
  }
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

const AUDIT_LEVELS = new Set(['info', 'low', 'moderate', 'high', 'critical'])

// The `update` and `audit` sections come from repo-controlled
// pnpm-workspace.yaml, which is parsed untyped — so their fields are validated
// here (the Rust config reader rejects the same malformed shapes at parse
// time). An invalid `audit.level` is especially worth catching: it would leave
// `pnpm audit` comparing severities against `undefined`, silently reporting no
// advisories.
function assertStringArray (value: unknown, settingName: string): asserts value is string[] {
  if (!Array.isArray(value) || value.some((item) => typeof item !== 'string')) {
    throw new PnpmError('INVALID_SETTING', `The "${settingName}" setting should be an array of strings, but got ${renderReceivedType(value)}`)
  }
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
