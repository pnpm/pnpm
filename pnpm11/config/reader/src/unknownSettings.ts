import { redactAndSanitize } from '@pnpm/error'
import type { WorkspaceManifest } from '@pnpm/workspace.workspace-manifest-reader'
import camelcase from 'camelcase'
import didYouMean, { ReturnTypeEnums } from 'didyoumean2'

import type { ConfigWithDeprecatedSettings } from './Config.js'
import { types } from './types.js'

/**
 * The structured settings of `pnpm-workspace.yaml`, which have no entry in
 * {@link types} because they never come in through a CLI flag.
 */
const TYPED_WORKSPACE_MANIFEST_KEYS = [
  'allowBuilds',
  'allowUnusedPatches',
  'allowedDeprecatedVersions',
  'audit',
  'auditConfig',
  'catalog',
  'catalogs',
  'configDependencies',
  'enableGlobalVirtualStore',
  'httpProxy',
  'httpsProxy',
  'ignoredOptionalDependencies',
  'namedRegistries',
  'nodeDownloadMirrors',
  'noProxy',
  'npmrcAuthFile',
  'overrides',
  'packageExtensions',
  'packages',
  'patchedDependencies',
  'peerDependencyRules',
  'pnprServer',
  'registries',
  'remoteSideEffectsCache',
  'requiredScripts',
  'sideEffectsCache',
  'supportedArchitectures',
  'tasks',
  'update',
  'updateConfig',
  'versioning',
  'virtualStoreType',
] as const satisfies ReadonlyArray<keyof WorkspaceManifest>

type ProofTypedWorkspaceManifestKeysAreExhaustive =
  (_: Record<typeof TYPED_WORKSPACE_MANIFEST_KEYS[number], unknown>) => Record<keyof WorkspaceManifest, unknown>

const _proofTypedWorkspaceManifestKeysAreExhaustive: ProofTypedWorkspaceManifestKeysAreExhaustive = (x) => x

/**
 * The {@link ConfigWithDeprecatedSettings} fields that neither {@link types}
 * nor {@link TYPED_WORKSPACE_MANIFEST_KEYS} carries — settings whose only
 * spelling is a camelCase config field (e.g. `catalogPrune`), and the reader's
 * own derived fields. {@link ProofKnownSettingKeysCoverConfig} forces a new
 * config field to be added here, so a setting introduced later cannot start
 * out being reported as unrecognized.
 */
const CONFIG_ONLY_SETTING_KEYS = [
  'allowNew',
  'auditIgnorePrune',
  'authConfig',
  'autoConfirmAllPrompts',
  'bin',
  'catalogPrune',
  'cleanupUnusedCatalogs',
  'configByUri',
  'enablePnp',
  'extraBinPaths',
  'extraEnv',
  'globalPkgDir',
  'globalPrefix',
  'ignoreCurrentSpecifiers',
  'maxSockets',
  'minimumReleaseAgeExcludePrune',
  'packageConfigs',
  'packageManagerNetworkConfig',
  'packageManagerRegistries',
  'pending',
  'pnpmExecPath',
  'pnpmHomeDir',
  'recursive',
  'registriesByPrefix',
  'registriesByScope',
  'registryOptionsByUrl',
  'reverse',
  'sideEffectsCacheRead',
  'sideEffectsCacheWrite',
  'tryLoadDefaultPnpmfile',
  'useGitBranchLockfile',
  'useLockfile',
  'useRunningStoreServer',
  'useStoreServer',
  'userConfig',
  'workspaceDir',
  'workspacePackagePatterns',
  'workspacePrefix',
] as const satisfies ReadonlyArray<keyof ConfigWithDeprecatedSettings>

const UNTYPED_WORKSPACE_SETTING_KEYS = [
  'confirmModulesPurge',
  'executionEnv',
  'ignoredBuiltDependencies',
  'neverBuiltDependencies',
  'onlyBuiltDependencies',
  'onlyBuiltDependenciesFile',
]

type KebabToCamelCase<S extends string> = S extends `${infer A}-${infer B}`
  ? `${A}${Capitalize<KebabToCamelCase<B>>}`
  : S

type KnownSettingKey =
  | KebabToCamelCase<keyof typeof types & string>
  | typeof TYPED_WORKSPACE_MANIFEST_KEYS[number]
  | typeof CONFIG_ONLY_SETTING_KEYS[number]

type ProofKnownSettingKeysCoverConfig =
  (_: Record<KnownSettingKey, unknown>) => Record<keyof ConfigWithDeprecatedSettings, unknown>

const _proofKnownSettingKeysCoverConfig: ProofKnownSettingKeysCoverConfig = (x) => x

const KNOWN_SETTING_KEYS: ReadonlySet<string> = new Set([
  ...TYPED_WORKSPACE_MANIFEST_KEYS,
  ...CONFIG_ONLY_SETTING_KEYS,
  ...UNTYPED_WORKSPACE_SETTING_KEYS,
  ...Object.keys(types).map((key) => camelcase(key, { locale: 'en-US' })),
])

/**
 * Whether {@link key}, given in either camelCase or kebab-case, names a
 * setting this version of pnpm reads from at least one config source. A key
 * failing this check is a typo or belongs to a different pnpm version, so it
 * gets {@link quoteAndAnnotateUnknown}'s warning instead of advice to move it
 * to another config file.
 */
export function isKnownSettingKey (key: string): boolean {
  return KNOWN_SETTING_KEYS.has(camelcase(key, { locale: 'en-US' }))
}

/**
 * Settings recognized by other supported pnpm release lines but not by this
 * one, so the warning can name the version that reads the key instead of
 * guessing at a typo. Maintained by hand: the lines are developed together in
 * this repository, so a line-exclusive setting is known at build time.
 */
const SETTINGS_OF_OTHER_PNPM_VERSIONS: Record<string, string> = {
  globalShims: 'pnpm v12',
}

const KNOWN_SETTING_KEYS_LIST = [...KNOWN_SETTING_KEYS]

/**
 * Renders unrecognized keys for a warning. The key comes from a config file a
 * repository may control, so it is sanitized before it reaches a terminal or a
 * CI log; the suggestion is one of pnpm's own setting names.
 */
export function quoteAndAnnotateUnknown (keys: string[]): string {
  return keys.map((key) => {
    const sanitized = redactAndSanitize(key)
    const camelKey = camelcase(sanitized, { locale: 'en-US' })
    const otherVersion = SETTINGS_OF_OTHER_PNPM_VERSIONS[camelKey]
    if (otherVersion) return `"${sanitized}" (a ${otherVersion} setting)`
    const suggestion = didYouMean(camelKey, KNOWN_SETTING_KEYS_LIST, { returnType: ReturnTypeEnums.FIRST_CLOSEST_MATCH })
    return suggestion ? `"${sanitized}" (did you mean "${suggestion}"?)` : `"${sanitized}"`
  }).join(', ')
}
