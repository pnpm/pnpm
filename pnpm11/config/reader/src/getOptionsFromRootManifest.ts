import path from 'node:path'

import { PnpmError } from '@pnpm/error'
import { globalWarn } from '@pnpm/logger'
import type {
  AllowedDeprecatedVersions,
  AuditConfig,
  AuditLevel,
  AuditSettings,
  PackageExtension,
  PeerDependencyRules,
  PnpmSettings,
  ProjectManifest,
  RegistryOptions,
  SupportedArchitectures,
  UpdateSettings,
  VirtualStoreType,
} from '@pnpm/types'
import { map as mapValues } from 'ramda'

import { translateRegistrySettings } from './registrySettings.js'
import { replaceEnvInSettings } from './replaceEnvInSettings.js'
import {
  assertBoolean,
  assertObjectSetting,
  assertOptionalBoolean,
  assertString,
  assertStringArray,
  assertStringRecord,
  renderReceivedType,
} from './settingAssertions.js'

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
  httpProxy?: string
  httpsProxy?: string
  /** The lookups the `registries` setting is split into. */
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

export function getOptionsFromPnpmSettings (
  manifestDir: string | undefined,
  pnpmSettings: PnpmSettings,
  manifestOrOpts?: ProjectManifest | GetOptionsFromPnpmSettingsOptions
): OptionsFromRootManifest {
  const opts = toGetOptionsFromPnpmSettingsOptions(manifestOrOpts)
  assertValidSideEffectsCacheSettings(pnpmSettings, opts.trustedSource ?? false)
  const settings: OptionsFromRootManifest = replaceEnvInSettings(pnpmSettings, {
    expandRequestDestinationEnv: opts.expandRequestDestinationEnv ?? false,
  })
  if (manifestDir != null && settings.scriptShell != null) {
    settings.scriptShell = resolveScriptShell(manifestDir, settings.scriptShell)
  }
  normalizeOverrides(settings, opts.manifest)
  if (settings.packageExtensions != null) {
    assertValidPackageExtensions(settings.packageExtensions)
  }
  resolvePatchedDependencies(settings, manifestDir)
  if (pnpmSettings.nodeDownloadMirrors != null) {
    assertStringRecord(pnpmSettings.nodeDownloadMirrors, 'nodeDownloadMirrors')
  }
  if (settings.allowBuilds != null) {
    assertValidAllowBuilds(settings.allowBuilds)
  }
  assertOptionalBoolean(settings.allowUnusedPatches, 'allowUnusedPatches')
  if (settings.ignoredOptionalDependencies != null) {
    assertStringArray(settings.ignoredOptionalDependencies, 'ignoredOptionalDependencies')
  }
  if (settings.requiredScripts != null) {
    assertStringArray(settings.requiredScripts, 'requiredScripts')
  }
  if (Object.hasOwn(settings, 'httpProxy')) {
    assertString(settings.httpProxy, 'httpProxy')
  }
  if (Object.hasOwn(settings, 'httpsProxy')) {
    assertString(settings.httpsProxy, 'httpsProxy')
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

function toGetOptionsFromPnpmSettingsOptions (
  manifestOrOpts: ProjectManifest | GetOptionsFromPnpmSettingsOptions | undefined
): GetOptionsFromPnpmSettingsOptions {
  if (isGetOptionsFromPnpmSettingsOptions(manifestOrOpts)) return manifestOrOpts
  return manifestOrOpts == null ? {} : { manifest: manifestOrOpts }
}

function assertValidSideEffectsCacheSettings (pnpmSettings: PnpmSettings, trustedSource: boolean): void {
  if (pnpmSettings.remoteSideEffectsCache != null) {
    assertValidSharedSideEffectsCache(pnpmSettings.remoteSideEffectsCache, trustedSource, 'remoteSideEffectsCache')
  }
  if (pnpmSettings.sideEffectsCache != null && typeof pnpmSettings.sideEffectsCache !== 'boolean') {
    assertValidSideEffectsCache(pnpmSettings.sideEffectsCache, trustedSource)
  }
}

function normalizeOverrides (settings: OptionsFromRootManifest, manifest: ProjectManifest | undefined): void {
  if (!settings.overrides) return
  assertValidOverrides(settings.overrides)
  if (Object.keys(settings.overrides).length === 0) {
    delete settings.overrides
    return
  }
  warnAboutDeprecatedVersionReferences(settings.overrides)
  if (manifest) {
    settings.overrides = mapValues(createVersionReferencesReplacer(manifest), settings.overrides)
  }
}

function resolvePatchedDependencies (settings: OptionsFromRootManifest, manifestDir: string | undefined): void {
  if (settings.patchedDependencies === undefined) return
  assertValidPatchedDependencies(settings.patchedDependencies)
  const patchedDependencies = { ...settings.patchedDependencies }
  settings.patchedDependencies = patchedDependencies
  for (const [dep, patchFile] of Object.entries(patchedDependencies)) {
    if (manifestDir == null || path.isAbsolute(patchFile)) continue
    patchedDependencies[dep] = path.join(manifestDir, patchFile)
  }
}

function resolveScriptShell (manifestDir: string, scriptShell: string): string {
  if (path.isAbsolute(scriptShell) || (!scriptShell.includes('/') && !scriptShell.includes('\\'))) {
    return scriptShell
  }
  return path.join(manifestDir, scriptShell)
}

/** The fields of a `tasks` entry that this version of pnpm reads. */
const TASK_SETTING_FIELDS = ['concurrency', 'dependsOn']

/**
 * The field of {@link TASK_SETTING_FIELDS} that {@link field} misspells, if
 * any. `pnpm-workspace.yaml` is one format across pnpm 11 and 12, so a field
 * this version does not read may simply be one only pnpm 12 acts on, and
 * rejecting every such field would make a pnpm 12 workspace unreadable here.
 * A field differing from one of ours only in case is the exception: no pnpm
 * version declares two settings that close, so it is a typo.
 */
function misspelledTaskSettingField (field: string): string | undefined {
  if (TASK_SETTING_FIELDS.includes(field)) return undefined
  return TASK_SETTING_FIELDS.find((known) => known.toLowerCase() === field.toLowerCase())
}

// The section feeds the task-graph builder of `pnpm -r run`, which reads it
// without further checks — a malformed entry has to be rejected here rather
// than surface as a scheduling bug far from the setting that produced it. A
// misspelled `dependsOn` is the costly one: the entry still exists, so the task
// takes the empty dependency list an entry without `dependsOn` declares, and
// runs before what it meant to wait for.
function assertValidTasks (tasks: unknown): asserts tasks is NonNullable<PnpmSettings['tasks']> {
  assertObjectSetting(tasks, 'tasks')
  for (const [taskName, task] of Object.entries(tasks as Record<string, unknown>)) {
    assertValidTask(`tasks['${taskName}']`, task)
  }
}

function assertValidTask (taskPath: string, task: unknown): void {
  assertObjectSetting(task, taskPath)
  assertNoMisspelledTaskFields(taskPath, task as Record<string, unknown>)
  const concurrency = (task as { concurrency?: unknown }).concurrency
  if (concurrency != null && (!Number.isInteger(concurrency) || (concurrency as number) < 1)) {
    throw new PnpmError('INVALID_SETTING',
      `The "${taskPath}.concurrency" setting should be a positive integer, but got ${JSON.stringify(concurrency)}`)
  }
  const dependsOn = (task as { dependsOn?: unknown }).dependsOn
  if (dependsOn == null) return
  assertStringArray(dependsOn, `${taskPath}.dependsOn`)
  for (const entry of dependsOn) {
    if (entry !== '' && entry !== '^') continue
    throw new PnpmError('INVALID_SETTING',
      `The "${taskPath}.dependsOn" setting contains an entry with no task name: ${JSON.stringify(entry)}`)
  }
}

function assertNoMisspelledTaskFields (taskPath: string, task: Record<string, unknown>): void {
  for (const field of Object.keys(task)) {
    const misspelled = misspelledTaskSettingField(field)
    if (misspelled == null) continue
    throw new PnpmError('INVALID_SETTING',
      `The "${taskPath}.${field}" setting is not a known task setting.`,
      { hint: `Did you mean "${misspelled}"?` })
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
    assertOnlyWorkspaceRemoteSideEffectsFields(settings, path)
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
    assertStringRecord(settings.buildEnv, `${path}.buildEnv`)
  }
}

function assertOnlyWorkspaceRemoteSideEffectsFields (
  settings: NonNullable<PnpmSettings['remoteSideEffectsCache']>,
  path: string
): void {
  const machineField = Object.keys(settings)
    .find((field) => !WORKSPACE_REMOTE_SIDE_EFFECTS_FIELDS.has(field))
  if (machineField == null) return
  throw new PnpmError(
    'WORKSPACE_REMOTE_SIDE_EFFECTS_TRUST',
    `${path}.${machineField} cannot be set by a workspace`,
    { hint: `Set it in the global config file (pnpm config set --location=global ${path}.${machineField} ...) or in the environment instead.` }
  )
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

function assertValidPatchedDependencies (patchedDependencies: unknown): asserts patchedDependencies is Record<string, string> {
  if (patchedDependencies == null || typeof patchedDependencies !== 'object' || Array.isArray(patchedDependencies)) {
    throw new PnpmError('INVALID_PATCHED_DEPENDENCY', `The patchedDependencies field should be an object, but got ${renderReceivedType(patchedDependencies)}`)
  }
  for (const [dep, patchFile] of Object.entries(patchedDependencies)) {
    if (typeof patchFile !== 'string') {
      throw new PnpmError('INVALID_PATCHED_DEPENDENCY', `The value of patchedDependencies.${dep} should be a string, but got ${renderReceivedType(patchFile)}`)
    }
  }
}

function assertValidAllowBuilds (allowBuilds: unknown): asserts allowBuilds is Record<string, boolean | string> {
  if (allowBuilds == null || typeof allowBuilds !== 'object' || Array.isArray(allowBuilds)) {
    throw new PnpmError('INVALID_ALLOW_BUILDS', `The allowBuilds field should be an object, but got ${renderReceivedType(allowBuilds)}`)
  }
  for (const [pkg, value] of Object.entries(allowBuilds)) {
    if (typeof value !== 'boolean' && typeof value !== 'string') {
      throw new PnpmError('INVALID_ALLOW_BUILDS', `The value of allowBuilds.${pkg} should be a boolean or string, but got ${renderReceivedType(value)}`)
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
    assertValidPackageExtension(`packageExtensions['${selector}']`, extension)
  }
}

function assertValidPackageExtension (extensionPath: string, extension: unknown): void {
  assertObjectSetting(extension, extensionPath)
  for (const field of PACKAGE_EXTENSION_DEPENDENCY_FIELDS) {
    const deps = (extension as Record<string, unknown>)[field]
    if (deps == null) continue
    assertStringRecord(deps, `${extensionPath}.${field}`)
  }
  const peerDependenciesMeta = (extension as Record<string, unknown>).peerDependenciesMeta
  if (peerDependenciesMeta == null) return
  assertObjectSetting(peerDependenciesMeta, `${extensionPath}.peerDependenciesMeta`)
  for (const [depName, meta] of Object.entries(peerDependenciesMeta as Record<string, unknown>)) {
    const metaPath = `${extensionPath}.peerDependenciesMeta.${depName}`
    assertObjectSetting(meta, metaPath)
    assertOptionalBoolean((meta as Record<string, unknown>).optional, `${metaPath}.optional`)
  }
}

const AUDIT_LEVELS = new Set(['info', 'low', 'moderate', 'high', 'critical'])

const VIRTUAL_STORE_TYPES = new Set<VirtualStoreType>(['global', 'project'])

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
  const newSpec = Object.hasOwn(dep, dependencyName) ? dep[dependencyName] : undefined
  if (newSpec) return newSpec
  throw new PnpmError(
    'CANNOT_RESOLVE_OVERRIDE_VERSION',
    `Cannot resolve version ${spec} in overrides. The direct dependencies don't have dependency "${dependencyName}".`
  )
}
