import path from 'node:path'

import { envReplace } from '@pnpm/config.env-replace'
import { PnpmError, redactAndSanitize } from '@pnpm/error'
import { globalWarn } from '@pnpm/logger'
import type {
  AllowedDeprecatedVersions,
  PackageExtension,
  PeerDependencyRules,
  PnpmSettings,
  ProjectManifest,
  RegistryOptions,
  SupportedArchitectures,
} from '@pnpm/types'
import normalizeRegistryUrl from 'normalize-registry-url'
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
} & Pick<PnpmSettings, 'configDependencies' | 'auditConfig' | 'pnprServer' | 'registryOptions' | 'updateConfig'>

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
  if (settings.packageExtensions != null) {
    assertValidPackageExtensions(settings.packageExtensions)
  }
  if (pnpmSettings.patchedDependencies) {
    settings.patchedDependencies = { ...pnpmSettings.patchedDependencies }
    for (const [dep, patchFile] of Object.entries(pnpmSettings.patchedDependencies)) {
      if (manifestDir == null || path.isAbsolute(patchFile)) continue
      settings.patchedDependencies[dep] = path.join(manifestDir, patchFile)
    }
  }
  if (settings.registryOptions != null) {
    settings.registryOptions = normalizeRegistryOptionsSetting(settings.registryOptions)
  }
  translateUpdateSettings(pnpmSettings, settings)
  translateAuditSettings(pnpmSettings, settings)

  return settings
}

const REGISTRY_SERVER_TYPES = new Set(['npm', 'artifactory'])

/**
 * Credentials and TLS material stay in `.npmrc`, which is not committed.
 * `registryOptions` lives in `pnpm-workspace.yaml`, which is, so accepting
 * these here would invite secrets into version control. Rejecting is better
 * than ignoring: a silently dropped `_authToken` reads as configured.
 */
const SECRET_REGISTRY_KEYS = new Set([
  '_auth', '_authToken', '_password', 'username', 'tokenHelper',
  'ca', 'cafile', 'cert', 'certfile', 'key', 'keyfile',
])

/**
 * Validates the `registryOptions` setting and keys it by normalized registry
 * URL, so a lookup by the registry a package resolved from matches the entry
 * however either one spelled the trailing slash.
 */
function normalizeRegistryOptionsSetting (
  registryOptions: Record<string, RegistryOptions>
): Record<string, RegistryOptions> {
  assertObjectSetting(registryOptions, 'registryOptions')
  const normalized: Record<string, RegistryOptions> = {}
  for (const [registry, options] of Object.entries(registryOptions)) {
    // The URL is user config that may carry `user:pass@` credentials, and it
    // is about to be interpolated into an error a terminal or CI log will show.
    const settingPath = `registryOptions['${redactRegistryUrl(registry)}']`
    assertObjectSetting(options, settingPath)
    for (const key of Object.keys(options)) {
      if (SECRET_REGISTRY_KEYS.has(key)) {
        throw new PnpmError('INVALID_SETTING',
          `The "${settingPath}.${key}" setting is not allowed in pnpm-workspace.yaml.`,
          { hint: `Set "//${redactRegistryUrl(registry).replace(/^(?:https?:)?\/\//, '')}:${key}" in an .npmrc file instead, so it is not committed.` })
      }
    }
    // `registryOptions` lives in the committed pnpm-workspace.yaml, and this
    // map already refuses credential fields for that reason; a credential in
    // the key is the same secret in the same file. A registry whose URL really
    // carries credentials should move them to .npmrc, which also makes the URL
    // here match the one pnpm resolves from.
    if (registryUrlHasUserinfo(registry)) {
      throw new PnpmError('INVALID_SETTING',
        `The "${settingPath}" key embeds credentials.`,
        { hint: 'Put them in an .npmrc file instead, so they are not committed.' })
    }
    const { serverType } = options
    if (serverType != null && !REGISTRY_SERVER_TYPES.has(serverType)) {
      throw new PnpmError('INVALID_SETTING',
        `The "${settingPath}.serverType" setting should be one of ${Array.from(REGISTRY_SERVER_TYPES).map((type) => `"${type}"`).join(', ')}, but got ${JSON.stringify(serverType)}`)
    }
    normalized[normalizeRegistryUrl(registry)] = options
  }
  return normalized
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
  const update = pnpmSettings.update
  if (update == null) return
  assertObjectSetting(update, 'update')
  if (pnpmSettings.updateConfig != null) {
    globalWarn('Both the "update" and "updateConfig" settings are set. The deprecated "updateConfig" setting is ignored in favor of "update".')
  }
  // The `update` section is authoritative when present: build the internal
  // `updateConfig` shape from it, superseding any deprecated `updateConfig`.
  const updateConfig: NonNullable<OptionsFromRootManifest['updateConfig']> = {}
  if (update.ignoreDeps != null) {
    assertStringArray(update.ignoreDeps, 'update.ignoreDeps')
    updateConfig.ignoreDependencies = update.ignoreDeps
  }
  if (update.changeset != null) {
    assertBoolean(update.changeset, 'update.changeset')
    updateConfig.changeset = update.changeset
  }
  if (update.githubActions != null) {
    assertBoolean(update.githubActions, 'update.githubActions')
    updateConfig.githubActions = update.githubActions
  }
  if (update.githubActionsServer != null) {
    assertString(update.githubActionsServer, 'update.githubActionsServer')
    updateConfig.githubActionsServer = update.githubActionsServer
  }
  settings.updateConfig = updateConfig
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
  assertObjectSetting(audit, 'audit')
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

const PACKAGE_EXTENSION_DEPENDENCY_FIELDS = ['dependencies', 'optionalDependencies', 'peerDependencies'] as const

// A malformed range here is not caught by anything downstream: the extender
// merges the value onto the manifest as is, and it only surfaces once peer
// resolution tries to read a version out of it, far away from the setting that
// produced it.
//
// A `null` field counts as absent rather than malformed — that is what a key
// left empty in YAML parses to, and what pacquet's `Option` fields accept.
function assertValidPackageExtensions (packageExtensions: unknown): asserts packageExtensions is Record<string, PackageExtension> {
  assertObjectSetting(packageExtensions, 'packageExtensions')
  for (const [selector, extension] of Object.entries(packageExtensions as Record<string, unknown>)) {
    const extensionPath = `packageExtensions['${selector}']`
    assertObjectSetting(extension, extensionPath)
    for (const field of PACKAGE_EXTENSION_DEPENDENCY_FIELDS) {
      const deps = (extension as Record<string, unknown>)[field]
      if (deps == null) continue
      assertObjectSetting(deps, `${extensionPath}.${field}`)
      for (const [depName, range] of Object.entries(deps as Record<string, unknown>)) {
        assertString(range, `${extensionPath}.${field}.${depName}`)
      }
    }
    const peerDependenciesMeta = (extension as Record<string, unknown>).peerDependenciesMeta
    if (peerDependenciesMeta == null) continue
    assertObjectSetting(peerDependenciesMeta, `${extensionPath}.peerDependenciesMeta`)
    for (const [depName, meta] of Object.entries(peerDependenciesMeta as Record<string, unknown>)) {
      const metaPath = `${extensionPath}.peerDependenciesMeta.${depName}`
      assertObjectSetting(meta, metaPath)
      const optional = (meta as Record<string, unknown>).optional
      if (optional == null) continue
      assertBoolean(optional, `${metaPath}.optional`)
    }
  }
}

function renderReceivedType (value: unknown): string {
  if (value === null) return 'null'
  if (Array.isArray(value)) return 'array'
  return typeof value
}

const AUDIT_LEVELS = new Set(['info', 'low', 'moderate', 'high', 'critical'])

// The `update`, `audit` and `packageExtensions` sections come from repo-controlled
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

function assertBoolean (value: unknown, settingName: string): asserts value is boolean {
  if (typeof value !== 'boolean') {
    throw new PnpmError('INVALID_SETTING', `The "${settingName}" setting should be a boolean, but got ${renderReceivedType(value)}`)
  }
}

function assertString (value: unknown, settingName: string): asserts value is string {
  if (typeof value !== 'string') {
    throw new PnpmError('INVALID_SETTING', `The "${settingName}" setting should be a string, but got ${renderReceivedType(value)}`)
  }
}

// Not an `asserts` guard on purpose: it only rejects malformed shapes at
// runtime, without narrowing away the section's declared type at the call site.
function assertObjectSetting (value: unknown, settingName: string): void {
  if (value == null || typeof value !== 'object' || Array.isArray(value)) {
    throw new PnpmError('INVALID_SETTING', `The "${settingName}" setting should be an object, but got ${renderReceivedType(value)}`)
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
    } else if (newKey === 'registryOptions') {
      // Keyed by registry URL rather than valued by one, so the request
      // destination is the key and the gate has to apply there.
      newSettings[newKey as keyof PnpmSettings] = (opts.expandRequestDestinationEnv
        ? replaceEnvInKeys(value)
        : copyEntriesWithoutEnvPlaceholderKeys(value)) as never
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

function replaceEnvInKeys (value: unknown): unknown {
  if (value == null || typeof value !== 'object' || Array.isArray(value)) return value
  const out: Record<string, unknown> = {}
  for (const [k, v] of Object.entries(value as Record<string, unknown>)) {
    out[envReplace(k, process.env)] = v
  }
  return out
}

function copyEntriesWithoutEnvPlaceholderKeys (value: unknown): unknown {
  if (value == null || typeof value !== 'object' || Array.isArray(value)) return value
  const out: Record<string, unknown> = {}
  for (const [k, v] of Object.entries(value as Record<string, unknown>)) {
    if (hasEnvPlaceholder(k)) continue
    out[k] = v
  }
  return out
}

/**
 * Whether the authority of `url` carries a `user:pass@` prefix. The authority
 * ends at the first `/`, `?`, or `#`, so a later `@` in the path is not one.
 *
 * Both the full form and the scheme-less `//host/` form count. The latter is
 * the shape `.npmrc` scopes settings with, so it is the one a user is most
 * likely to reach for here.
 */
function registryUrlHasUserinfo (url: string): boolean {
  return userinfoEnd(url) !== undefined
}

/**
 * The offset just past the `user:pass@` of `url`, or `undefined` when its
 * authority carries none. Splitting it out keeps the detection and the
 * redaction below agreeing on what the authority is.
 */
function userinfoEnd (url: string): number | undefined {
  const authorityStart = authorityStartOf(url)
  if (authorityStart === undefined) return undefined
  const authority = url.slice(authorityStart)
  const authorityEnd = authority.search(/[/?#]/)
  const at = (authorityEnd === -1 ? authority : authority.slice(0, authorityEnd)).lastIndexOf('@')
  return at === -1 ? undefined : authorityStart + at + 1
}

/**
 * Where the authority of `url` begins, or `undefined` if it has none.
 *
 * The scheme is anchored at the start rather than found by searching for the
 * first `://`: a `://` inside the path (`//host/a://b`) would otherwise be
 * taken for the separator, and the real authority — credentials and all —
 * would go unexamined.
 */
function authorityStartOf (url: string): number | undefined {
  const schemeEnd = url.indexOf('://')
  if (schemeEnd !== -1 && SCHEME.test(url.slice(0, schemeEnd))) return schemeEnd + '://'.length
  if (url.startsWith('//')) return '//'.length
  return undefined
}

const SCHEME = /^[a-z][a-z0-9+.-]*$/i

/**
 * `url` with any `user:pass@` removed, safe to put in a message.
 *
 * {@link redactAndSanitize} only recognizes an authority after a `://`, and
 * deliberately so: it runs over arbitrary prose, where a bare `//` is more
 * often a comment or a path than a URL. Here the string is known to be a
 * registry URL, so the scheme-less `//host/` form can be handled too.
 */
function redactRegistryUrl (url: string): string {
  const authorityStart = authorityStartOf(url)
  const end = userinfoEnd(url)
  if (authorityStart === undefined || end === undefined) return redactAndSanitize(url)
  return redactAndSanitize(`${url.slice(0, authorityStart)}${url.slice(end)}`)
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
