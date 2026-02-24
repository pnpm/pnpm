import normalizeRegistryUrl from 'normalize-registry-url'

/**
 * If {@link text} starts with {@link oldPrefix}, replace it with {@link newPrefix}.
 * Otherwise, return `undefined`.
 */
const replacePrefix = <NewPrefix extends string> (
  text: string,
  oldPrefix: string,
  newPrefix: NewPrefix
): `${NewPrefix}${string}` | undefined =>
  text.startsWith(oldPrefix)
    ? text.replace(oldPrefix, newPrefix) as `${NewPrefix}${string}`
    : undefined

/**
 * If {@link text} already ends with {@link suffix}, return it.
 * Otherwise, append {@link suffix} to {@link text} and return it.
 */
const ensureSuffix = <
  Text extends string,
  Suffix extends string
> (text: Text, suffix: Suffix): `${Text}${Suffix}` =>
  text.endsWith(suffix) ? text as `${Text}${Suffix}` : `${text}${suffix}`

/**
 * Protocols currently supported.
 */
type SupportedRegistryScheme = 'http' | 'https'

/**
 * A registry URL that has been normalized to match its corresponding {@link RegistryConfigKey}.
 */
export type NormalizedRegistryUrl = `${SupportedRegistryScheme}://${string}/`

/**
 * A config key of a registry url is a key on the `.npmrc` file. This key starts with
 * a "//" prefix followed by a hostname and the rest of the URI and ends with a "/".
 * They usually specify authentication information.
 */
export type RegistryConfigKey = `//${string}/`

export interface SupportedRegistryUrlInfo {
  normalizedUrl: NormalizedRegistryUrl
  longestConfigKey: RegistryConfigKey
}

/**
 * If the {@link registryUrl} is an HTTP or an HTTPS registry url, return the longest
 * {@link RegistryConfigKey} that corresponds to the registry url and a {@link NormalizedRegistryUrl}
 * that matches it.
 */
export function parseSupportedRegistryUrl (registryUrl: string): SupportedRegistryUrlInfo | undefined {
  registryUrl = normalizeRegistryUrl(registryUrl)
  const keyPrefix = replacePrefix(registryUrl, 'http://', '//') ?? replacePrefix(registryUrl, 'https://', '//')
  if (!keyPrefix) return undefined
  const normalizedUrl = ensureSuffix(registryUrl, '/') as NormalizedRegistryUrl
  const longestConfigKey = ensureSuffix(keyPrefix, '/')
  return { normalizedUrl, longestConfigKey }
}

/**
 * This value is used for termination check in {@link allRegistryConfigKeys} only.
 * It is not actually a valid {@link RegistryConfigKey}.
 */
const EMPTY_REGISTRY_CONFIG_KEY: RegistryConfigKey = '///'

/**
 * Generate all {@link RegistryConfigKey} of the same hostname from the longest to the shortest,
 * including {@link longest} itself.
 */
export function * allRegistryConfigKeys (longest: RegistryConfigKey): Generator<RegistryConfigKey, void, void> {
  if (!longest.startsWith('//')) {
    throw new RangeError(`The string ${JSON.stringify(longest)} is not a valid registry config key`)
  }
  if (longest === EMPTY_REGISTRY_CONFIG_KEY) {
    throw new RangeError('Registry config key cannot be without hostname')
  }
  if (longest.length <= EMPTY_REGISTRY_CONFIG_KEY.length) return
  yield longest
  const next = longest.replace(/[^/]*\/$/, '') as RegistryConfigKey
  yield * allRegistryConfigKeys(next)
}
