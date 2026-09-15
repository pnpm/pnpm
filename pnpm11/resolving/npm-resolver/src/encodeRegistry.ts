import { Buffer } from 'node:buffer'
import util from 'node:util'

import { createHexHash } from '@pnpm/crypto.hash'
import { PnpmError, redactAndSanitize, redactUrlForDisplay } from '@pnpm/error'

/**
 * Bytes a registry key carries verbatim. Everything else is percent-escaped,
 * so a `%` or `+` in the result is always one this module wrote, and the key
 * can never contain a path separator, a character Windows rejects in a
 * filename, or a glob metacharacter — the cache commands interpolate the key
 * straight into a glob pattern, and `pnpm cache delete` erases whatever that
 * pattern matches.
 */
const VERBATIM_BYTES = new Set<number>(
  Array.from('abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789._-', (char) => char.charCodeAt(0))
)

/**
 * Separates the scheme from the host. Every key begins with one, and a scheme
 * cannot contain a `%`, so the first occurrence always ends the scheme.
 */
const SCHEME_SEPARATOR = '%3A+'

/**
 * Separates the host from the path. A percent-escape, so that nothing an
 * escaped component can spell is mistaken for it.
 */
const PATH_SEPARATOR = '%2F'

/** Separates the key from the hash that disambiguates a mixed-case path. */
const HASH_SEPARATOR = '%5F'

/**
 * The 255-byte limit on one filename that ext4, APFS and NTFS share. A
 * registry path long enough to exceed it would make every mirror read miss and
 * every mirror write fail, so such a key collapses to its own hash: still one
 * directory per registry, just no longer readable.
 */
const MAX_KEY_LENGTH = 255

/**
 * Directory name under the metadata cache root that holds a registry's
 * mirrored packuments, shaped
 * `<scheme>%3A+<host>[+<port>][%2F<path>][%5F<hash>]`.
 *
 * `+` joins the port to the host and the path segments to one another, and
 * every other occurrence of it is escaped, as is every character that could
 * spell one of the separators — so no two registry URLs share a directory.
 * They must not: a shared directory lets the resolver answer from another
 * registry's versions, integrity hashes and tarball URLs, which surfaces as
 * `ERR_PNPM_TARBALL_URL_MISMATCH` when the lockfile is verified.
 *
 * The scheme is part of the key because `http` and `https` at one host and
 * path are two different trust domains: metadata served over `http` can be
 * rewritten in transit, and must never be handed to a resolution configured
 * for `https`. Including it also puts a `%` in every key, and a key written
 * before this change was a bare URL host — which can never contain one — so no
 * new key can land on a stale directory.
 *
 * The port is dropped when it is the scheme's default and leading and trailing
 * slashes are trimmed, so the same registry spelled `https://r/`,
 * `https://r:443` and `https://r:443/` keeps one cache rather than three. A
 * path that is not all lowercase gets a sha256 suffix, the guard
 * `encodePkgName` applies to package names, because HFS+ and NTFS would
 * otherwise merge `…/Team` into `…/team`. A trailing `.` is escaped because
 * Win32 strips one, which would alias `…/foo.` onto `…/foo`. A key that would
 * not fit a 255-byte filename is replaced by its own hash.
 *
 * `registry` must be a URL with a host; a resolver always has both, so
 * anything else is malformed config and throws
 * `ERR_PNPM_INVALID_REGISTRY_URL` or `ERR_PNPM_MISSING_REGISTRY_HOST`.
 */
export function encodeRegistry (registry: string): string {
  let url: URL
  try {
    url = new URL(registry)
  } catch (err: unknown) {
    // `err` is not attached as the cause: Node's ERR_INVALID_URL carries the
    // raw registry — credentials and all — in its `input` property, so
    // anything that serializes the cause would undo the redaction here.
    const reason = util.types.isNativeError(err) ? err.message : String(err)
    throw new PnpmError('INVALID_REGISTRY_URL', `Failed to parse registry URL "${redactAndSanitize(registry)}": ${redactAndSanitize(reason)}`)
  }
  if (url.hostname === '') {
    throw new PnpmError('MISSING_REGISTRY_HOST', `Registry URL "${redactUrlForDisplay(registry)}" has no host`)
  }
  const scheme = escapeComponent(url.protocol.slice(0, -1))
  const host = url.port === '' ? escapeComponent(url.hostname) : `${escapeComponent(url.hostname)}+${url.port}`
  const segments = pathSegments(url.pathname)
  let key = `${scheme}${SCHEME_SEPARATOR}${host}`
  if (segments.length > 0) {
    key += `${PATH_SEPARATOR}${segments.map(escapeComponent).join('+')}`
    const pathname = segments.join('/')
    if (pathname !== pathname.toLowerCase()) {
      key += `${HASH_SEPARATOR}${createHexHash(pathname)}`
    }
  }
  if (key.endsWith('.')) {
    key = `${key.slice(0, -1)}%2E`
  }
  return key.length <= MAX_KEY_LENGTH ? key : createHexHash(key)
}

/**
 * The registry a key made by {@link encodeRegistry} came from. The sha256 that
 * separates two paths differing only in case is not part of the registry, so
 * it is dropped.
 *
 * The result is the registry in the trailing-slashed form the resolver
 * normalizes to, which is what makes it the exact inverse: `…%2F` names
 * `https://r//`, one slash more than `…` alone.
 *
 * `pnpm cache view` labels its output with this. A key carrying no scheme
 * separator decodes to the `host[:port]` it names, so a cache root holding
 * both shapes still labels every entry, and anything that does not decode is
 * returned unchanged rather than throwing.
 */
export function decodeRegistry (registryKey: string): string {
  const schemeIndex = registryKey.indexOf(SCHEME_SEPARATOR)
  const authority = schemeIndex === -1 ? registryKey : registryKey.slice(schemeIndex + SCHEME_SEPARATOR.length)
  const separatorIndex = authority.indexOf(PATH_SEPARATOR)
  const host = separatorIndex === -1 ? authority : authority.slice(0, separatorIndex)
  try {
    const decodedHost = decodeURIComponent(host.replaceAll('+', ':'))
    if (schemeIndex === -1) return decodedHost
    const scheme = decodeURIComponent(registryKey.slice(0, schemeIndex))
    if (separatorIndex === -1) return `${scheme}://${decodedHost}/`
    const rest = authority.slice(separatorIndex + PATH_SEPARATOR.length)
    const hashIndex = rest.indexOf(HASH_SEPARATOR)
    const segments = (hashIndex === -1 ? rest : rest.slice(0, hashIndex)).split('+')
    return `${scheme}://${decodedHost}/${segments.map(decodeURIComponent).join('/')}/`
  } catch {
    return registryKey
  }
}

function escapeComponent (component: string): string {
  let escaped = ''
  for (const byte of Buffer.from(component, 'utf8')) {
    escaped += VERBATIM_BYTES.has(byte)
      ? String.fromCharCode(byte)
      : `%${byte.toString(16).toUpperCase().padStart(2, '0')}`
  }
  return escaped
}

/**
 * The path a registry addresses, as the segments between its slashes.
 *
 * The only spelling difference that does not reach the registry is a missing
 * trailing slash, because the resolver appends one to a registry configured
 * without it — so `…/a` and `…/a/` share a cache. Every other slash is
 * significant: `https://r/`, `https://r//` and `https://r///` request
 * `/lodash`, `//lodash` and `///lodash` respectively, so they are three
 * registries and get three directories, carrying one, two and three segments.
 */
function pathSegments (pathname: string): string[] {
  // Every pathname opens with the `/` that {@link PATH_SEPARATOR} stands for,
  // so its empty head is dropped; a trailing `/` is the one slash the resolver
  // normalizes away, so its empty tail goes with it.
  const segments = pathname.split('/').slice(1)
  if (pathname.endsWith('/')) segments.pop()
  return segments
}
