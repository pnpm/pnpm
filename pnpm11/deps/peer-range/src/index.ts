import { validRange } from 'semver'

export function isValidPeerRange (version: string): boolean {
  // we use `includes` instead of `startsWith` because `workspace:*` and `catalog:*` could be a part of a wider version range expression
  return typeof validRange(version) === 'string' || version.includes('workspace:') || version.includes('catalog:')
}

/**
 * Whether a `peerDependencies` value is accepted at install time.
 *
 * A value is accepted when it is either a valid peer range (semver,
 * `workspace:`, or `catalog:`) or any specifier that carries a
 * protocol/registry scheme — a named-registry spec (`work:5.x.x`), an `npm:`
 * alias, or a `file:`/`git`/URL spec. Such specifiers are desugared during
 * resolution: {@link getPeerVersionRange} yields the semver range they are
 * matched against, while the original specifier still drives auto-installation.
 *
 * Bare `name@version` typos, which have no scheme and are not valid semver, are
 * still rejected — they are almost always a mistaken attempt to pin a peer to a
 * specific version.
 */
export function isAcceptablePeerSpec (version: string): boolean {
  return isValidPeerRange(version) || version.includes(':')
}

/**
 * The semver range a resolved version is checked against for a peer dependency.
 *
 * `workspace:` prefixes are stripped; a named-registry or `npm:` specifier
 * contributes its version body (`work:5.x.x` → `5.x.x`, `npm:bar@^5` → `^5`);
 * any other non-semver specifier (git, file, URL) becomes `*`, so the peer is
 * satisfied by any version while its original specifier still selects the
 * package to install. Valid semver ranges and `catalog:` specs are returned
 * unchanged.
 */
export function getPeerVersionRange (version: string): string {
  if (isValidPeerRange(version)) {
    return version.replace(/^workspace:/, '')
  }
  const colon = version.indexOf(':')
  if (colon > 0) {
    const body = version.slice(colon + 1)
    if (validRange(body) != null) return body
    const at = body.lastIndexOf('@')
    if (at > 0 && validRange(body.slice(at + 1)) != null) return body.slice(at + 1)
  }
  return '*'
}
