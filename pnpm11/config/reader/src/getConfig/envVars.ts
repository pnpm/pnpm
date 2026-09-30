import type { VirtualStoreType } from '@pnpm/types'
import kebabCase from 'lodash.kebabcase'
import normalizeRegistryUrl from 'normalize-registry-url'
import { omit } from 'ramda'

import { parseEnvVars, type Schema } from '../env.js'
import { types } from '../types.js'
import type { ConfigBuildState } from './configBuildState.js'
import { applySideEffectsCacheDeclaration } from './sideEffectsCache.js'

export function getProcessEnv (env: string): string | undefined {
  return process.env[env] ??
    process.env[env.toUpperCase()] ??
    process.env[env.toLowerCase()]
}

// Look up a `pnpm_config_<key>` env var, accepting both lowercase and
// uppercase forms. Used for env vars that need to be read before the
// general parseEnvVars pass, such as those that affect which .npmrc file
// is loaded.
export function readEnvVar (env: NodeJS.ProcessEnv, key: string): string | undefined {
  const value = env[`pnpm_config_${key}`] ?? env[`PNPM_CONFIG_${key.toUpperCase()}`]
  return value !== '' ? value : undefined
}

// Same shape as readEnvVar but for the `npm_config_<key>` family. Used as a
// low-priority compatibility shim so that npm-style env vars (e.g.
// NPM_CONFIG_USERCONFIG written by actions/setup-node) keep working.
export function readNpmEnvVar (env: NodeJS.ProcessEnv, key: string): string | undefined {
  const value = env[`npm_config_${key}`] ?? env[`NPM_CONFIG_${key.toUpperCase()}`]
  return value !== '' ? value : undefined
}

/**
 * `globalPkgDir` and `bin` are derived from the global directories before the
 * full `PNPM_CONFIG_*` pass runs, so the two settings they are built from
 * have to come off the environment first; that pass sets them again to the
 * same values. The CLI outranks the environment, exactly as it does there.
 */
export function applyGlobalDirEnvVars ({ cliOptions, env, explicitlySetKeys, pnpmConfig }: ConfigBuildState): void {
  for (const { key, value } of parseEnvVars(getGlobalDirSchema, env)) {
    if ((key !== 'globalDir' && key !== 'globalBinDir') || typeof value !== 'string') continue
    if (isSetOnCommandLine(cliOptions, key)) continue
    pnpmConfig[key] = value
    explicitlySetKeys.add(key)
  }
}

function getGlobalDirSchema (schemaKey: string): Schema | undefined {
  return schemaKey === 'global-dir' || schemaKey === 'global-bin-dir' ? types[schemaKey] : undefined
}

/** The `maxSockets` values the environment carried, under either spelling. */
export interface MaxSocketsFromEnv {
  maxsocketsFromEnv?: number
  maxSocketsFromEnv?: number
}

interface HeldBackEnvSettings extends MaxSocketsFromEnv {
  virtualStoreTypeFromEnv?: VirtualStoreType
}

export function applyEnvVarSettings (state: ConfigBuildState): MaxSocketsFromEnv {
  const { cliOptions, env, explicitlySetKeys, pnpmConfig } = state
  const heldBack: HeldBackEnvSettings = {}
  for (const { key, value } of parseEnvVars(key => ENV_PNPM_TYPES[key as keyof typeof ENV_PNPM_TYPES], env)) {
    // undefined means that the env key was defined, but its value couldn't be parsed according to the schema
    // TODO: should we throw some error or print some warning here?
    if (value === undefined) continue
    if (isSetOnCommandLine(cliOptions, key)) continue
    if (holdBackEnvSetting(heldBack, key, value)) continue
    applyEnvSetting(state, key, value)
  }
  if (heldBack.virtualStoreTypeFromEnv != null) {
    pnpmConfig.enableGlobalVirtualStore = heldBack.virtualStoreTypeFromEnv === 'global'
    explicitlySetKeys.add('enableGlobalVirtualStore')
  }
  return { maxsocketsFromEnv: heldBack.maxsocketsFromEnv, maxSocketsFromEnv: heldBack.maxSocketsFromEnv }
}

const ENV_PNPM_TYPES = {
  ...omit([
    // npm interprets leading-zero values as octal, while the Number schema does not.
    'umask',
  ], types),
  // `types` carries npm's `maxsockets` spelling alone, so without this
  // entry `PNPM_CONFIG_MAX_SOCKETS` — the canonical setting name, spelled
  // the way the environment spells every other camelCase setting — would
  // match no schema and be dropped. Env-only: the CLI flag and
  // `pnpm config` keys keep npm's spelling.
  'max-sockets': Number,
}

function isSetOnCommandLine (cliOptions: Record<string, unknown>, key: string): boolean {
  return Object.hasOwn(cliOptions, key) || Object.hasOwn(cliOptions, kebabCase(key))
}

/**
 * Keeps back the values that are resolved only after the whole environment
 * was read. Returns whether {@link key} was one of them.
 */
function holdBackEnvSetting (heldBack: HeldBackEnvSettings, key: string, value: unknown): boolean {
  switch (key) {
  // Held back rather than assigned: the rest of pnpm reads the boolean
  // spelling, and applying the translation after the loop is what makes
  // the canonical key win over `PNPM_CONFIG_ENABLE_GLOBAL_VIRTUAL_STORE`
  // whichever order the two arrive in.
    case 'virtualStoreType':
      heldBack.virtualStoreTypeFromEnv = value as VirtualStoreType
      return true
      // The two spellings of `maxSockets` the environment can carry, held
      // back rather than assigned so the fold after the loop can keep the
      // environment ranked above the config files whichever order the two
      // arrive in.
    case 'maxsockets':
      heldBack.maxsocketsFromEnv = value as number
      return true
    case 'maxSockets':
      heldBack.maxSocketsFromEnv = value as number
      return true
    default:
      return false
  }
}

function applyEnvSetting ({ explicitlySetKeys, pnpmConfig }: ConfigBuildState, key: string, value: unknown): void {
  // The environment can only spell the boolean, and a plain assignment
  // would drop a remote tier a config file declared under the object form.
  if (key === 'sideEffectsCache') {
    applySideEffectsCacheDeclaration(pnpmConfig, value)
    explicitlySetKeys.add(key)
    return
  }

  // @ts-expect-error -- the value's type depends on which key this is
  pnpmConfig[key] = value
  explicitlySetKeys.add(key)

  if (key === 'registry') {
    if (typeof value !== 'string') {
      throw new TypeError(`Unexpected type of registry, expecting a string but received ${JSON.stringify(value)}`)
    }
    pnpmConfig.registriesByScope.default = normalizeRegistryUrl(value)
    pnpmConfig.packageManagerRegistries!.default = normalizeRegistryUrl(value)
  }
}
