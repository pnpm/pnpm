import path from 'node:path'

import { envReplace } from '@pnpm/config.env-replace'
import { PnpmError, redactAndSanitize } from '@pnpm/error'
import { globalWarn } from '@pnpm/logger'
import {
  type AllowedDeprecatedVersions,
  type AuditConfig,
  type AuditLevel,
  type AuditSettings,
  DEFAULT_REGISTRY_SCOPE,
  type PackageExtension,
  type PeerDependencyRules,
  type PnpmSettings,
  type ProjectManifest,
  type RegistryDeclaration,
  type RegistryOptions,
  type SupportedArchitectures,
  type UpdateSettings,
  type VirtualStoreType,
} from '@pnpm/types'
import normalizeRegistryUrl from 'normalize-registry-url'
import { map as mapValues } from 'ramda'

import { quoteAndJoin } from './quoteAndJoin.js'

export type OptionsFromRootManifest = {
  scriptShell?: string
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
  /** The three lookups the `registries` setting is split into. */
  registriesByScope?: Record<string, string>
  registriesByPrefix?: Record<string, string>
  registryOptionsByUrl?: Record<string, RegistryOptions>
  auditIgnorePrune?: boolean
} & Pick<PnpmSettings, 'configDependencies' | 'auditConfig' | 'pnprServer' | 'remoteSideEffectsCache' | 'sideEffectsCache' | 'tasks' | 'updateConfig'>

interface GetOptionsFromPnpmSettingsOptions {
  /**
   * The settings come from a file the repository does not control (the global
   * config yaml), so they may carry the fields
   * {@link SHARED_SIDE_EFFECTS_TRUST_KEYS} names. Defaults to `false`, which
   * treats the source as repo-controlled.
   */
  trustedSource?: boolean
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
  if (pnpmSettings.remoteSideEffectsCache != null) {
    assertValidSharedSideEffectsCache(pnpmSettings.remoteSideEffectsCache, opts.trustedSource ?? false, 'remoteSideEffectsCache')
  }
  if (pnpmSettings.sideEffectsCache != null && typeof pnpmSettings.sideEffectsCache !== 'boolean') {
    assertValidSideEffectsCache(pnpmSettings.sideEffectsCache, opts.trustedSource ?? false)
  }
  const settings: OptionsFromRootManifest = replaceEnvInSettings(pnpmSettings, {
    expandRequestDestinationEnv: opts.expandRequestDestinationEnv ?? false,
  })
  if (manifestDir != null && settings.scriptShell != null) {
    settings.scriptShell = resolveScriptShell(manifestDir, settings.scriptShell)
  }
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
  translateRegistrySettings(settings)
  translateUpdateSettings(pnpmSettings, settings)
  translateAuditSettings(pnpmSettings, settings)
  translateVirtualStoreType(pnpmSettings, settings)
  if (settings.tasks != null) {
    assertValidTasks(settings.tasks)
  }

  return settings
}

function resolveScriptShell (manifestDir: string, scriptShell: string): string {
  if (path.isAbsolute(scriptShell) || (!scriptShell.includes('/') && !scriptShell.includes('\\'))) {
    return scriptShell
  }
  return path.join(manifestDir, scriptShell)
}

/** The fields a `tasks` entry may carry. Anything else is a typo. */
const TASK_SETTING_FIELDS = new Set(['concurrency', 'dependsOn'])

// The section feeds the task-graph builder of `pnpm -r run`, which reads it
// without further checks — a malformed entry has to be rejected here rather
// than surface as a scheduling bug far from the setting that produced it.
function assertValidTasks (tasks: unknown): asserts tasks is NonNullable<PnpmSettings['tasks']> {
  assertObjectSetting(tasks, 'tasks')
  for (const [taskName, task] of Object.entries(tasks as Record<string, unknown>)) {
    const taskPath = `tasks['${taskName}']`
    assertObjectSetting(task, taskPath)
    for (const field of Object.keys(task as Record<string, unknown>)) {
      if (TASK_SETTING_FIELDS.has(field)) continue
      throw new PnpmError('INVALID_SETTING',
        `The "${taskPath}.${field}" setting is not a known task setting.`,
        { hint: `A task declares ${quoteAndJoin([...TASK_SETTING_FIELDS])}.` })
    }
    const concurrency = (task as { concurrency?: unknown }).concurrency
    if (concurrency != null && (!Number.isInteger(concurrency) || (concurrency as number) < 1)) {
      throw new PnpmError('INVALID_SETTING',
        `The "${taskPath}.concurrency" setting should be a positive integer, but got ${JSON.stringify(concurrency)}`)
    }
    const dependsOn = (task as { dependsOn?: unknown }).dependsOn
    if (dependsOn == null) continue
    assertStringArray(dependsOn, `${taskPath}.dependsOn`)
    for (const entry of dependsOn) {
      if (entry !== '' && entry !== '^') continue
      throw new PnpmError('INVALID_SETTING',
        `The "${taskPath}.dependsOn" setting contains an entry with no task name: ${JSON.stringify(entry)}`)
    }
  }
}

/**
 * The signing trust root stays outside the repository: a workspace declares
 * which organization and packages are eligible and nothing else — see
 * {@link WORKSPACE_REMOTE_SIDE_EFFECTS_FIELDS}. Letting it set `publish` would
 * turn a key the machine holds for its own builds into a signing oracle any
 * cloned repository could aim at a registry of its choosing.
 *
 * The rest of the section is consumed without further checks by the
 * install-time lookup, so a malformed shape has to be rejected here rather than
 * surface as a type error deep inside the hydration path.
 */
function assertValidSideEffectsCache (
  settings: Exclude<NonNullable<PnpmSettings['sideEffectsCache']>, boolean>,
  trustedSource: boolean
): void {
  assertObjectSetting(settings, 'sideEffectsCache')
  assertOptionalBoolean(settings.read, 'sideEffectsCache.read')
  assertOptionalBoolean(settings.write, 'sideEffectsCache.write')
  if (settings.remote != null) {
    assertValidSharedSideEffectsCache(settings.remote, trustedSource, 'sideEffectsCache.remote')
  }
}

function assertValidSharedSideEffectsCache (
  settings: NonNullable<PnpmSettings['remoteSideEffectsCache']>,
  trustedSource: boolean,
  path: string
): void {
  assertObjectSetting(settings, path)
  if (!trustedSource) {
    const machineField = Object.keys(settings)
      .find((field) => !WORKSPACE_REMOTE_SIDE_EFFECTS_FIELDS.has(field))
    if (machineField != null) {
      throw new PnpmError(
        'WORKSPACE_REMOTE_SIDE_EFFECTS_TRUST',
        `${path}.${machineField} cannot be set by a workspace`,
        { hint: `Set it in the global config file (pnpm config set --location=global ${path}.${machineField} ...) or in the environment instead.` }
      )
    }
  }
  if (settings.org != null) {
    assertString(settings.org, `${path}.org`)
  }
  if (settings.organization != null) {
    assertString(settings.organization, `${path}.organization`)
  }
  if (settings.packages != null) {
    assertStringArray(settings.packages, `${path}.packages`)
  }
  assertOptionalBoolean(settings.publish, `${path}.publish`)
  for (const field of ['keyId', 'builderId', 'imageDigest', 'architectureBaseline', 'privateKey'] as const) {
    if (settings[field] == null) continue
    assertString(settings[field], `${path}.${field}`)
  }
  if (settings.buildEnv != null) {
    assertObjectSetting(settings.buildEnv, `${path}.buildEnv`)
    for (const [name, value] of Object.entries(settings.buildEnv)) {
      assertString(value, `${path}.buildEnv.${name}`)
    }
  }
}

/**
 * The only fields of `remoteSideEffectsCache` a committed file may contribute.
 *
 * Everything else describes the act of signing — which key signs, what
 * provenance the signature attests, and whether to publish at all — so it
 * belongs to the machine holding the key. Listing what a repository may set,
 * rather than what it may not, keeps a field added later machine-only until
 * someone decides otherwise.
 */
const WORKSPACE_REMOTE_SIDE_EFFECTS_FIELDS: ReadonlySet<string> = new Set(['org', 'organization', 'packages'])

const REGISTRY_SERVER_TYPES = new Set(['npm', 'artifactory'])

/**
 * Credentials and TLS material stay in `.npmrc`, which is not committed.
 * `registries` lives in `pnpm-workspace.yaml`, which is, so accepting these
 * here would invite secrets into version control. Rejecting is better than
 * ignoring: a silently dropped `_authToken` reads as configured.
 */
const SECRET_REGISTRY_KEYS = new Set([
  '_auth', '_authToken', '_password', 'username', 'tokenHelper',
  'ca', 'cafile', 'cert', 'certfile', 'key', 'keyfile',
])

/** The fields a `registries` entry may carry. Anything else is a typo. */
const REGISTRY_DECLARATION_FIELDS = new Set(['serverType', 'supportsTimeField', 'scopes', 'prefix'])

/**
 * Turns the settings that name registries into the three lookups the rest of
 * pnpm reads: the scope-routed URLs, the `<prefix>:`-addressed URLs, and the
 * per-registry options. The declaration map itself does not travel.
 */
function translateRegistrySettings (settings: OptionsFromRootManifest): void {
  // Both settings are read under the names a user writes and removed, because
  // the keys the rest of pnpm reads are the lookups they feed.
  const written = settings as Pick<PnpmSettings, 'namedRegistries' | 'registries'>
  const deprecatedPrefixes = written.namedRegistries
  delete written.namedRegistries
  const declarations = written.registries
  delete written.registries

  const lookups = readRegistryDeclarations(declarations)
  if (deprecatedPrefixes != null && Object.keys(lookups.registriesByPrefix).length > 0) {
    globalWarn('Both the "registries" and "namedRegistries" settings declare registry prefixes. The deprecated "namedRegistries" setting is only read for prefixes "registries" does not declare.')
  }
  // A prefix a `registries` entry declares wins: the two spell the same thing.
  const registriesByPrefix = { ...deprecatedPrefixes, ...lookups.registriesByPrefix }
  if (Object.keys(registriesByPrefix).length > 0) settings.registriesByPrefix = registriesByPrefix
  if (Object.keys(lookups.registriesByScope).length > 0) settings.registriesByScope = lookups.registriesByScope
  if (Object.keys(lookups.registryOptionsByUrl).length > 0) settings.registryOptionsByUrl = lookups.registryOptionsByUrl
}

/**
 * Reads the `registries` setting and inverts the routes each entry declares,
 * or returns `undefined` when there is nothing to invert.
 *
 * A registry is declared once, keyed by its URL, because every fact in an
 * entry is a fact about that server: how it lays out tarball URLs, and which
 * routes reach it. The lookups are the inverse of the routes, because a scope
 * resolves to exactly one registry while a registry serves many.
 *
 * The older shape — a map whose values are all strings — is the scope lookup
 * already, and is left in place as it stands.
 */
function readRegistryDeclarations (declarations: PnpmSettings['registries']): RegistryLookups {
  const empty: RegistryLookups = { registriesByScope: {}, registriesByPrefix: {}, registryOptionsByUrl: {} }
  if (declarations == null) return empty
  assertObjectSetting(declarations, 'registries')
  const entries = Object.entries(declarations)
  const scopeRoutes = entries.filter(([, declaration]) => typeof declaration === 'string')
  if (scopeRoutes.length === entries.length) {
    assertNoUrlKeyedScopeRoutes(scopeRoutes.map(([scope]) => scope))
    return { ...empty, registriesByScope: Object.fromEntries(scopeRoutes as Array<[string, string]>) }
  }
  if (scopeRoutes.length > 0) {
    throw new PnpmError('INVALID_SETTING',
      `The "registries" setting mixes registry declarations with "<scope>: <url>" entries (${quoteAndJoin(scopeRoutes.map(([scope]) => scope))}).`,
      { hint: 'Key every entry by registry URL and list the scopes routed to it under "scopes".' })
  }
  return splitRegistryDeclarations(entries as Array<[string, RegistryDeclaration]>)
}

/**
 * A scope routes to a registry, so a URL in that position routes nothing and
 * would sit there inert. It is the declaration shape, half-written.
 */
function assertNoUrlKeyedScopeRoutes (scopes: string[]): void {
  for (const scope of scopes) {
    if (!looksLikeRegistryUrl(scope)) continue
    throw new PnpmError('INVALID_SETTING',
      `The "registries['${redactRegistryUrl(scope)}']" entry is a string.`,
      { hint: 'A registry URL keys a declaration, e.g. { serverType: "artifactory" }. A string value routes a scope, and a URL is not a scope.' })
  }
}

interface RegistryLookups {
  /** Scope-routed URLs, normalized, with `@` under its `default` key. */
  registriesByScope: Record<string, string>
  /**
   * Prefix-addressed URLs, deliberately kept as written: a named registry's
   * URL is what a lockfile's recorded tarball URLs are matched against, so
   * normalizing it would change what an existing lockfile verifies against.
   */
  registriesByPrefix: Record<string, string>
  registryOptionsByUrl: Record<string, RegistryOptions>
}

/** Validates the declarations and inverts their routes into the lookups. */
function splitRegistryDeclarations (entries: Array<[string, RegistryDeclaration]>): RegistryLookups {
  const lookups: RegistryLookups = { registriesByScope: {}, registriesByPrefix: {}, registryOptionsByUrl: {} }
  for (const [registry, declaration] of entries) {
    // The URL is user config that may carry `user:pass@` credentials, and it
    // is about to be interpolated into an error a terminal or CI log will show.
    const settingPath = `registries['${redactRegistryUrl(registry)}']`
    assertObjectSetting(declaration, settingPath)
    assertKnownRegistryFields(declaration, registry, settingPath)
    // The map lives in the committed pnpm-workspace.yaml, and it already
    // refuses credential fields for that reason; a credential in the key is
    // the same secret in the same file. A registry whose URL really carries
    // credentials should move them to .npmrc, which also makes the URL here
    // match the one pnpm resolves from.
    if (registryUrlHasUserinfo(registry)) {
      throw new PnpmError('INVALID_SETTING',
        `The "${settingPath}" key embeds credentials.`,
        { hint: 'Put them in an .npmrc file instead, so they are not committed.' })
    }
    // Normalized for the lookups keyed by URL, so that a registry a package
    // resolved from matches its declaration however either one spelled the
    // trailing slash.
    const normalizedRegistry = normalizeRegistryUrl(registry)
    const { serverType, supportsTimeField, scopes, prefix } = declaration
    if (serverType != null && !REGISTRY_SERVER_TYPES.has(serverType)) {
      throw new PnpmError('INVALID_SETTING',
        `The "${settingPath}.serverType" setting should be one of ${quoteAndJoin([...REGISTRY_SERVER_TYPES])}, but got ${JSON.stringify(serverType)}`)
    }
    if (supportsTimeField != null) {
      assertBoolean(supportsTimeField, `${settingPath}.supportsTimeField`)
    }
    if (serverType != null || supportsTimeField != null) {
      lookups.registryOptionsByUrl[normalizedRegistry] = {
        ...(serverType != null ? { serverType } : {}),
        ...(supportsTimeField != null ? { supportsTimeField } : {}),
      }
    }
    if (scopes != null) {
      assertStringArray(scopes, `${settingPath}.scopes`)
      for (const scope of scopes) {
        if (!scope.startsWith(DEFAULT_REGISTRY_SCOPE)) {
          throw new PnpmError('INVALID_SETTING',
            `The "${settingPath}.scopes" setting should list "@"-prefixed scopes, but got ${JSON.stringify(scope)}`,
            { hint: `A bare "${DEFAULT_REGISTRY_SCOPE}" is the scope-less default registry.` })
        }
        const scopeKey = scope === DEFAULT_REGISTRY_SCOPE ? 'default' : scope
        const routed = lookups.registriesByScope[scopeKey]
        if (routed != null && routed !== normalizedRegistry) {
          throw new PnpmError('INVALID_SETTING',
            `The scope ${JSON.stringify(scope)} is routed to two registries: ${quoteAndJoin([routed, normalizedRegistry].map(redactAndSanitize))}.`)
        }
        lookups.registriesByScope[scopeKey] = normalizedRegistry
      }
    }
    if (prefix != null) {
      assertString(prefix, `${settingPath}.prefix`)
      const declaredBy = lookups.registriesByPrefix[prefix]
      if (declaredBy != null) {
        throw new PnpmError('INVALID_SETTING',
          `The prefix ${JSON.stringify(prefix)} is declared by two registries: ${quoteAndJoin([declaredBy, registry].map(redactAndSanitize))}.`)
      }
      lookups.registriesByPrefix[prefix] = registry
    }
  }
  return lookups
}

/**
 * A misspelled field would otherwise sit there doing nothing, and a credential
 * field would sit in a committed file. Both are refused here rather than by
 * the yaml parser, which renders the offending source line verbatim under its
 * errors — printing the very credential being refused.
 */
function assertKnownRegistryFields (declaration: RegistryDeclaration, registry: string, settingPath: string): void {
  for (const field of Object.keys(declaration)) {
    if (REGISTRY_DECLARATION_FIELDS.has(field)) continue
    if (SECRET_REGISTRY_KEYS.has(field)) {
      throw new PnpmError('INVALID_SETTING',
        `The "${settingPath}.${field}" setting is not allowed in pnpm-workspace.yaml.`,
        { hint: `Set "//${redactRegistryUrl(registry).replace(/^(?:https?:)?\/\//, '')}:${field}" in an .npmrc file instead, so it is not committed.` })
    }
    throw new PnpmError('INVALID_SETTING',
      `The "${settingPath}.${field}" setting is not a known registry setting.`,
      { hint: `A registry declares ${quoteAndJoin([...REGISTRY_DECLARATION_FIELDS])}.` })
  }
}

function looksLikeRegistryUrl (key: string): boolean {
  return key.includes('://') || key.startsWith('//')
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
  // `auditIgnorePrune` is derived from `audit.ignorePrune` below; a raw
  // top-level key of that name is not a setting in either CLI.
  delete settings.auditIgnorePrune
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
  if (audit.ignorePrune != null) {
    assertBoolean(audit.ignorePrune, 'audit.ignorePrune')
    settings.auditIgnorePrune = audit.ignorePrune
  }
}

/**
 * The `update` settings the CLI acts on, re-joined from the internal
 * `updateConfig` shape {@link translateUpdateSettings} splits the section
 * into — the view `pnpm config get update` prints. `undefined` when nothing
 * is set.
 */
export function toUpdateSettings (updateConfig: OptionsFromRootManifest['updateConfig']): UpdateSettings | undefined {
  if (updateConfig == null) return undefined
  const update: UpdateSettings = {
    ...(updateConfig.ignoreDependencies != null ? { ignoreDeps: updateConfig.ignoreDependencies } : {}),
    ...(updateConfig.changeset != null ? { changeset: updateConfig.changeset } : {}),
    ...(updateConfig.githubActions != null ? { githubActions: updateConfig.githubActions } : {}),
    ...(updateConfig.githubActionsServer != null ? { githubActionsServer: updateConfig.githubActionsServer } : {}),
  }
  return Object.keys(update).length > 0 ? update : undefined
}

/**
 * The `audit` settings the CLI acts on, re-joined from the internal
 * `auditConfig` / `auditLevel` / `auditIgnorePrune` settings
 * {@link translateAuditSettings} splits the section into — the view
 * `pnpm config get audit` prints. An empty ignore list reads as unset.
 * `undefined` when nothing is set.
 */
export function toAuditSettings ({ auditConfig, auditLevel, auditIgnorePrune }: { auditConfig?: AuditConfig, auditLevel?: AuditLevel, auditIgnorePrune?: boolean }): AuditSettings | undefined {
  const ignore = auditConfig?.ignoreGhsas
  const audit: AuditSettings = {
    ...(auditLevel != null ? { level: auditLevel } : {}),
    ...(ignore != null && ignore.length > 0 ? { ignore } : {}),
    ...(auditIgnorePrune != null ? { ignorePrune: auditIgnorePrune } : {}),
  }
  return Object.keys(audit).length > 0 ? audit : undefined
}

/**
 * Translates the user-facing `virtualStoreType` setting into the internal
 * `enableGlobalVirtualStore` boolean the rest of pnpm reads, and removes the
 * raw key from the returned options.
 *
 * `virtualStoreType` and `enableGlobalVirtualStore` are two spellings of one
 * setting, and a manifest may carry either or both. The canonical one wins,
 * silently: spelling a setting two ways is not itself a mistake worth
 * warning about. Same rule as `catalogPrune` over `cleanupUnusedCatalogs`.
 */
function translateVirtualStoreType (pnpmSettings: PnpmSettings, settings: OptionsFromRootManifest): void {
  delete (settings as { virtualStoreType?: unknown }).virtualStoreType
  const virtualStoreType = pnpmSettings.virtualStoreType
  if (virtualStoreType == null) return
  if (!VIRTUAL_STORE_TYPES.has(virtualStoreType)) {
    throw new PnpmError('INVALID_SETTING', `The "virtualStoreType" setting should be one of ${Array.from(VIRTUAL_STORE_TYPES).join(', ')}, but got ${JSON.stringify(virtualStoreType)}`)
  }
  ;(settings as { enableGlobalVirtualStore?: boolean }).enableGlobalVirtualStore = virtualStoreType === 'global'
}

function isGetOptionsFromPnpmSettingsOptions (
  value: ProjectManifest | GetOptionsFromPnpmSettingsOptions | undefined
): value is GetOptionsFromPnpmSettingsOptions {
  return value != null && ('expandRequestDestinationEnv' in value || 'manifest' in value || 'trustedSource' in value)
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

const VIRTUAL_STORE_TYPES = new Set<VirtualStoreType>(['global', 'project'])

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

function assertOptionalBoolean (value: unknown, settingName: string): void {
  if (value == null) return
  assertBoolean(value, settingName)
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
    } else if (newKey === 'namedRegistries' || (newKey === 'registries' && isScopeRouteMap(value))) {
      newSettings[newKey as keyof PnpmSettings] = (opts.expandRequestDestinationEnv
        ? replaceEnvInStringValues(value)
        : copyStringValuesWithoutEnvPlaceholders(value)) as never
    } else if (newKey === 'registries') {
      // A declaration map is keyed by registry URL rather than valued by one,
      // so the request destination is the key and the gate has to apply there.
      newSettings[newKey as keyof PnpmSettings] = (opts.expandRequestDestinationEnv
        ? replaceEnvInKeys(value)
        : copyEntriesWithoutEnvPlaceholderKeys(value)) as never
    } else {
      newSettings[newKey as keyof PnpmSettings] = value
    }
  }
  return newSettings
}

/**
 * Whether a `registries` value is the older `<scope>: <url>` shape rather than
 * a map of declarations. The registry URL is the value in the one and the key
 * in the other, so which one carries the request destination — and so which
 * one the env-placeholder gate applies to — follows from this.
 *
 * A map that mixes the two is a declaration map here; {@link
 * translateRegistrySettings} rejects it with a message about the mixing.
 */
function isScopeRouteMap (value: unknown): boolean {
  if (value == null || typeof value !== 'object' || Array.isArray(value)) return false
  const values = Object.values(value as Record<string, unknown>)
  return values.length > 0 && values.every((entry) => typeof entry === 'string')
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
