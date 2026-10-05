import { redactAndSanitize } from '@pnpm/error'
import camelcase from 'camelcase'
import kebabCase from 'lodash.kebabcase'

import type { Config, ConfigContext } from '../Config.js'
import { isConfigFileKey } from '../configFileKey.js'
import { CONFIG_CONTEXT_KEY_SET } from './configContext.js'

/**
 * A YAML-language-server schema association, not a setting; tools put it in
 * config files pnpm reads, so it must not trip the unknown-setting warnings.
 */
export const SCHEMA_DIRECTIVE_KEY = '$schema'

/**
 * Settings the project `pnpm-workspace.yaml` does not contribute to
 * `self-update`'s config.
 *
 * `self-update` replaces the pnpm binary every later install runs through, so
 * a repository must not get a say in whether it may be replaced. Each of these
 * is dangerous in both directions: a release-age cooldown lowered waives the
 * protection the user configured, raised it pins the machine to the installed
 * pnpm — including past a release that fixes a vulnerability in it; a
 * `trustPolicy` turned off accepts a pnpm release whose trust evidence the
 * user meant to reject, turned on blocks the update the same way; and `ci`
 * decides whether an immature pick may be confirmed at the keyboard at all.
 * Unlike a blocked dependency upgrade, those decisions follow the user out of
 * the repository. The policy therefore comes from the built-in defaults, the
 * global config yaml, the environment, and CLI flags only.
 */
export const SELF_UPDATE_SKIPPED_SETTINGS = [
  'ci',
  'minimumReleaseAge',
  'minimumReleaseAgeExclude',
  'minimumReleaseAgeIgnoreMissingTime',
  'minimumReleaseAgeStrict',
  'trustPolicy',
  'trustPolicyExclude',
  'trustPolicyIgnoreAfter',
] as const satisfies ReadonlyArray<keyof Config>

/**
 * Where the machine keeps what it holds across runs, which no project chooses.
 *
 * A repository setting one would redirect where pnpm writes: `pnpm login`'s
 * `auth.ini`, `pnpm setup`'s PATH entry, the bins `pnpm install` links.
 */
const MACHINE_LOCATION_KEYS = [
  'configDir',
  'globalBinDir',
  'globalDir',
  'globalPkgDir',
  'npmrcAuthFile',
  'pnpmHomeDir',
  'stateDir',
  'userconfig',
] as const satisfies ReadonlyArray<keyof (Config & ConfigContext)>

/**
 * The directories the current command reads and writes in.
 *
 * The reader resolves these from the command line and the cwd, and it needs
 * them before it can find a manifest at all, so a manifest cannot supply them.
 */
const CURRENT_RUN_LOCATION_KEYS = [
  'bin',
  'dir',
  'rootProjectManifestDir',
  'workspaceDir',
] as const satisfies ReadonlyArray<keyof (Config & ConfigContext)>

/**
 * Which credentials pnpm sends, and to whom.
 *
 * The reader assembles these from the trusted config sources. `userConfig` is
 * the parsed contents of the user's `.npmrc`, so it carries credentials rather
 * than the path `npmrcAuthFile` holds.
 */
const CREDENTIAL_KEYS = [
  'authConfig',
  'userConfig',
  'configByUri',
  'packageManagerNetworkConfig',
  'packageManagerRegistries',
] as const satisfies ReadonlyArray<keyof (Config & ConfigContext)>

/**
 * Which scope a `pnpm login` claims for the machine.
 *
 * The granted token is recorded as a route in the global `auth.ini`, which
 * outranks the user's own `~/.npmrc` in every project on the machine — so the
 * choice is the user's, not a repository's.
 */
const LOGIN_TARGET_KEYS = [
  'scope',
] as const satisfies ReadonlyArray<keyof Config>

/**
 * Keys a project's `pnpm-workspace.yaml` does not contribute.
 *
 * `cacheDir` and `storeDir` are deliberately absent: those name caches a
 * project may legitimately place.
 */
type ProjectManifestSkippedKey =
  | typeof MACHINE_LOCATION_KEYS[number]
  | typeof CURRENT_RUN_LOCATION_KEYS[number]
  | typeof CREDENTIAL_KEYS[number]
  | typeof LOGIN_TARGET_KEYS[number]

/** Every key a caller of {@link addSettingsFromWorkspaceManifestToConfig} may skip. */
export type SkippableKey =
  | ProjectManifestSkippedKey
  | typeof SELF_UPDATE_SKIPPED_SETTINGS[number]
  | typeof GLOBAL_CONFIG_ONLY_SKIPPED_KEYS[number]

export const PROJECT_MANIFEST_SKIPPED_KEYS: ReadonlySet<ProjectManifestSkippedKey> = new Set([
  ...MACHINE_LOCATION_KEYS,
  ...CURRENT_RUN_LOCATION_KEYS,
  ...CREDENTIAL_KEYS,
  ...LOGIN_TARGET_KEYS,
])

/**
 * The refused keys the global config file does not accept either.
 *
 * That file's own contents are already filtered by {@link isConfigFileKey},
 * but the CLI options are merged in again alongside them, so without this a
 * `--config.config-dir` would land back on a key the reader resolves for
 * itself, and only for the users who happen to have a `config.yaml`.
 */
/**
 * Only the layout half of a `registries` entry is workspace-only: it decides
 * which tarball URLs are omitted from the lockfile, so a machine-local setting
 * would make one developer write a lockfile their collaborators read back with
 * a different layout. The routes to the registry are a legitimate global
 * preference, which is why the whole `registries` key is not refused here.
 */
const GLOBAL_CONFIG_ONLY_SKIPPED_KEYS = ['registryOptionsByUrl'] as const satisfies ReadonlyArray<keyof Config>

export const GLOBAL_CONFIG_SKIPPED_KEYS: ReadonlySet<SkippableKey> = new Set([
  ...[...PROJECT_MANIFEST_SKIPPED_KEYS].filter((key) => !isConfigFileKey(kebabCase(key))),
  ...GLOBAL_CONFIG_ONLY_SKIPPED_KEYS,
])


/**
 * Whether a project's `pnpm-workspace.yaml` would ignore this camelCase key.
 * See {@link PROJECT_MANIFEST_SKIPPED_KEYS}.
 */
export function isProjectManifestSkippedKey (camelKey: string): boolean {
  const keys: ReadonlySet<string> = PROJECT_MANIFEST_SKIPPED_KEYS
  return keys.has(camelKey)
}

/**
 * Whether a project's `pnpm-workspace.yaml` drops {@link key}, given in either
 * camelCase or kebab-case, whether as a value a project may not contribute or
 * as the reader's own bookkeeping.
 *
 * Shared by the warnings so that they cannot disagree on what was dropped.
 */
export function isRefusedByAProjectManifest (key: string): boolean {
  const camelKey = camelcase(key, { locale: 'en-US' })
  return isProjectManifestSkippedKey(camelKey) || CONFIG_CONTEXT_KEY_SET.has(camelKey)
}

/**
 * The global config file key that sets a refused setting, where it is not that
 * setting's own name.
 */
const GLOBAL_EQUIVALENT_KEYS: Record<string, string> = {
  /** Derived from the global bin directory. */
  bin: 'global-bin-dir',
  /** Derived from the global directory. */
  globalPkgDir: 'global-dir',
  /**
   * Accepted under its own name, but never read back: the user-level `.npmrc`
   * comes from `npmrcAuthFile` or `--userconfig`, so its own name would send
   * the user to a command that changes nothing.
   */
  userconfig: 'npmrc-auth-file',
}

/**
 * Where {@link camelKey} can be set, for a key a project manifest refuses.
 *
 * Lives here rather than in the config command so that the reader's warnings
 * and the command's errors cannot drift into naming different routes for the
 * same setting.
 */
export function whereRefusedKeyBelongs (camelKey: string): string {
  if (camelKey === 'dir') return 'Pass --dir on the command line instead'
  const kebabKey = GLOBAL_EQUIVALENT_KEYS[camelKey] ?? kebabCase(camelKey)
  if (isConfigFileKey(kebabKey)) {
    return `Set it for the machine instead: pnpm config set --global ${kebabKey}`
  }
  return 'This is not a pnpm setting'
}

function quoteRefusedKey (key: string): string {
  const sanitized = redactAndSanitize(key)
  return `"${sanitized}" (${whereRefusedKeyBelongs(camelcase(sanitized, { locale: 'en-US' }))})`
}

export function quoteAndExplain (keys: string[]): string {
  return keys.map(quoteRefusedKey).join(', ')
}

/** Renders keys pnpm only reads in camelCase, naming the spelling that works. */
export function quoteAndSuggestCamelCase (keys: string[]): string {
  return keys.map((key) => {
    const sanitized = redactAndSanitize(key)
    return `"${sanitized}" (use "${camelcase(sanitized, { locale: 'en-US' })}")`
  }).join(', ')
}
