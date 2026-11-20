import { isError, PnpmError } from '@pnpm/error'
import type { RemoteSideEffectsCacheSettings, SideEffectsCacheSettings } from '@pnpm/types'

import type { Config, ConfigContext } from '../Config.js'

/**
 * Resolves the accepted spellings into the fields consumers read.
 *
 * `sideEffectsCache: true` sets reading and writing together,
 * `sideEffectsCacheReadonly` is `read` without `write`, and
 * `remoteSideEffectsCache` is `remote`; `sideEffectsCache` as an object wins
 * over any of them on a field they both set. Its `read` and `write` default to
 * enabled, so declaring only `remote` does not quietly switch the local cache
 * off.
 */
export function resolveSideEffectsCache (pnpmConfig: Config): void {
  const declared = pnpmConfig.sideEffectsCache
  const settings = typeof declared === 'object' && declared != null ? declared : undefined
  const shorthand = typeof declared === 'boolean' ? declared : undefined
  const readonly = pnpmConfig.sideEffectsCacheReadonly === true
  pnpmConfig.sideEffectsCacheRead = settings != null
    ? settings.read ?? true
    // `sideEffectsCacheReadonly: true` with `sideEffectsCache: false` is how
    // pacquet documents a read-only view, so either flag enables reading.
    : (shorthand ?? false) || readonly
  pnpmConfig.sideEffectsCacheWrite = settings != null
    ? settings.write ?? true
    // `sideEffectsCacheReadonly` reads as blocking writes and is documented as
    // doing so.
    : readonly ? false : shorthand
  // Combined here rather than as each source is read, so that the canonical
  // spelling wins on a field both set no matter which order they appeared in.
  if (settings?.remote != null) {
    pnpmConfig.remoteSideEffectsCache = {
      ...pnpmConfig.remoteSideEffectsCache,
      ...settings.remote,
    }
  }
}

/**
 * Rewrites `organization` to `org` before the two remote spellings are merged.
 *
 * Merging first and resolving after would compare a field named `org` on one
 * side against `organization` on the other, so neither would take precedence
 * over the other and whichever key happened to be named `org` would win.
 */
export function withCanonicalOrg (remote: RemoteSideEffectsCacheSettings | undefined): RemoteSideEffectsCacheSettings | undefined {
  if (remote?.organization == null) return remote
  const { organization, ...rest } = remote
  return { ...rest, org: rest.org ?? organization }
}

/**
 * The environment is the last word on the remote side-effects cache: it is
 * where a CI runner injects the signing material that must not be committed,
 * and where a build job flips publication on for one invocation.
 *
 * These are read here rather than by their consumers so the values reach the
 * installer as ordinary settings, and so `pnpm config list` can show them.
 */
export function applyRemoteSideEffectsCacheEnv (
  pnpmConfig: Config & ConfigContext,
  env: NodeJS.ProcessEnv
): void {
  const settings: Partial<RemoteSideEffectsCacheSettings> = {}
  const publish = readSideEffectsCacheEnv(env, 'PUBLISH')
  if (publish != null) {
    settings.publish = publish.value === 'true'
  }
  for (const [field, suffix] of SIDE_EFFECTS_CACHE_REMOTE_ENV_STRINGS) {
    const read = readSideEffectsCacheEnv(env, suffix)
    if (read != null) settings[field] = read.value
  }
  for (const [field, suffix] of SIDE_EFFECTS_CACHE_REMOTE_ENV_JSON) {
    const read = readSideEffectsCacheEnv(env, suffix)
    if (read == null) continue
    settings[field] = parseStringValuedJsonObject(read.value, read.variable)
  }
  if (Object.keys(settings).length === 0) return
  pnpmConfig.remoteSideEffectsCache = {
    ...pnpmConfig.remoteSideEffectsCache,
    ...settings,
  } as RemoteSideEffectsCacheSettings
  pnpmConfig.explicitlySetKeys.add('remoteSideEffectsCache')
}

const SIDE_EFFECTS_CACHE_REMOTE_ENV_STRINGS = [
  ['keyId', 'KEY_ID'],
  ['builderId', 'BUILDER_ID'],
  ['imageDigest', 'IMAGE_DIGEST'],
  ['architectureBaseline', 'ARCHITECTURE_BASELINE'],
  ['privateKey', 'PRIVATE_KEY'],
] as const satisfies ReadonlyArray<[keyof RemoteSideEffectsCacheSettings, string]>

const SIDE_EFFECTS_CACHE_REMOTE_ENV_JSON = [
  ['buildEnv', 'BUILD_ENV'],
  ['trustedKeys', 'TRUSTED_KEYS'],
] as const satisfies ReadonlyArray<[keyof RemoteSideEffectsCacheSettings, string]>

/**
 * Reads one field of the remote tier from the environment, under the name that
 * matches the setting and under the one that matched its older spelling.
 *
 * A machine configured for `remoteSideEffectsCache` keeps working; a machine
 * setting both gets the name that matches the setting it is configuring.
 */
function readSideEffectsCacheEnv (env: NodeJS.ProcessEnv, suffix: string): { value: string, variable: string } | undefined {
  // The name comes back with the value because a malformed one is reported by
  // name, and naming a variable the user did not set sends them looking for it.
  for (const variable of [`PNPM_SIDE_EFFECTS_CACHE_REMOTE_${suffix}`, `PNPM_REMOTE_SIDE_EFFECTS_CACHE_${suffix}`]) {
    const value = env[variable]
    if (value != null) return { value, variable }
  }
  return undefined
}

function parseStringValuedJsonObject (value: string, variable: string): Record<string, string> {
  let parsed: unknown
  try {
    parsed = JSON.parse(value)
  } catch (err: unknown) {
    throw new PnpmError('INVALID_REMOTE_SIDE_EFFECTS_ENV',
      `${variable} is not valid JSON: ${isError(err) ? err.message : String(err)}`)
  }
  if (parsed == null || typeof parsed !== 'object' || Array.isArray(parsed) || !Object.values(parsed).every((item) => typeof item === 'string')) {
    throw new PnpmError('INVALID_REMOTE_SIDE_EFFECTS_ENV', `${variable} must be a JSON object with string values`)
  }
  return parsed as Record<string, string>
}

/**
 * Merge one source's `sideEffectsCache` declaration into the config, later
 * sources landing on top of earlier ones.
 *
 * A boolean says whether to read and write. It says nothing about the remote
 * tier, so one declared by an earlier source survives it — but it has to
 * survive as a remote tier rather than by turning the boolean into an object,
 * which would move the whole declaration onto the object branch of
 * {@link resolveSideEffectsCache} and take it out of reach of
 * `sideEffectsCacheReadonly`.
 */
export function applySideEffectsCacheDeclaration (pnpmConfig: Config, value: unknown): void {
  const previous = typeof pnpmConfig.sideEffectsCache === 'object' && pnpmConfig.sideEffectsCache != null
    ? pnpmConfig.sideEffectsCache
    : undefined
  if (typeof value === 'boolean') {
    if (previous?.remote != null) {
      pnpmConfig.remoteSideEffectsCache = {
        ...pnpmConfig.remoteSideEffectsCache,
        ...withCanonicalOrg(previous.remote),
      }
    }
    pnpmConfig.sideEffectsCache = value
  } else if (value != null) {
    const declared = value as SideEffectsCacheSettings
    const remote = previous?.remote != null || declared.remote != null
      ? { ...previous?.remote, ...withCanonicalOrg(declared.remote) }
      : undefined
    pnpmConfig.sideEffectsCache = { ...previous, ...declared, remote }
  }
}
