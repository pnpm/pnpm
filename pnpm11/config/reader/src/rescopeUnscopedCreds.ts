import { nerfDart } from '@pnpm/config.registry-auth-key'
import normalizeRegistryUrl from 'normalize-registry-url'

import { npmDefaults } from './npmDefaults.js'

// Per-registry rc keys that, when written without a `//host/` prefix, fall
// through to whatever default registry the merged config settles on. We
// rewrite each such key to its URL-scoped form at load time, pinning it to
// the `registry=` value declared in the same source. A later layer can
// still override the merged registry, but it cannot pull along a credential
// or client certificate authored for a different host.
//
// Two groups:
// * auth keys — `_authToken` etc. Pinned to prevent credential leaks. npm
//   rejects these unscoped since npm@9 (ERR_INVALID_AUTH); pnpm keeps them
//   working but warns so users migrate before a future major drops support.
// * client certificate keys — `cert`/`key` (inline PEM). Pinned to prevent
//   a client certificate (and the identity it carries) being presented to
//   the wrong host. The `certfile`/`keyfile` path variants are not in
//   `NPM_AUTH_SETTINGS`, so unscoped forms never reach the merged config
//   in the first place — only the URL-scoped `//host/:certfile=...` and
//   `//host/:keyfile=...` forms are honored, and those are already pinned
//   to their authoring registry by construction.
//
// `ca`/`cafile` are intentionally left unscoped-by-default: they're trust
// anchors, not credentials, and corporate MITM-proxy setups rely on them
// applying globally to every HTTPS request. The default registry override
// can't weaponize an unscoped CA (the attacker would need a cert signed
// by it), so the same pinning isn't warranted.
const UNSCOPED_RESCOPABLE_KEYS = [
  '_authToken', '_auth', 'username', '_password', 'tokenHelper',
  'cert', 'key',
] as const

// Rewrite any unscoped per-registry keys in `source` to their URL-scoped
// equivalents (`//host[:port]/path/:<key>=...`) using `source.registry` —
// or the builtin default registry if the source doesn't declare its own.
// A URL-scoped key for the same
// registry already present in `source` wins; we never overwrite an
// explicit scoped value.
export function rescopeUnscopedCreds (
  source: Record<string, unknown>,
  sourceLabel: string,
  warnings: string[]
): Record<string, unknown> {
  // Bail early if there's nothing to rescope. This skips the nerfDart call
  // when a source like the builtin pnpmrc has only a `registry=` line —
  // rescoping there would do nothing anyway.
  if (!UNSCOPED_RESCOPABLE_KEYS.some(key => key in source)) {
    return source
  }
  const nerfedRegistry = nerfSourceRegistry(source)
  if (nerfedRegistry == null) {
    dropUnscopedCreds(source, sourceLabel, warnings)
    return source
  }
  const rescoped = pinUnscopedCreds(source, nerfedRegistry)
  if (rescoped.length > 0) {
    warnings.push(`Unscoped per-registry settings (${rescoped.join(', ')}) in "${sourceLabel}" are deprecated. ` +
      `pnpm pinned them to "${nerfedRegistry}" for this run, but a future release will stop supporting unscoped per-registry settings. ` +
      `Write them as "${nerfedRegistry}:${rescoped[0]}=..." instead.`)
  }
  return source
}

function nerfSourceRegistry (source: Record<string, unknown>): string | undefined {
  const rawRegistry = typeof source.registry === 'string' && source.registry !== '' ? source.registry : null
  const fallbackRegistry = rawRegistry ?? npmDefaults.registry
  try {
    return nerfDart(normalizeRegistryUrl(fallbackRegistry))
  } catch {
    return undefined
  }
}

// `registry=` resolved to something `URL` can't parse — often an
// unresolved `${VAR}` placeholder that left the string empty. Drop the
// unscoped keys (a bare token is unsafe to bind anywhere) and warn.
function dropUnscopedCreds (source: Record<string, unknown>, sourceLabel: string, warnings: string[]): void {
  const dropped = UNSCOPED_RESCOPABLE_KEYS.filter(key => key in source)
  for (const key of dropped) delete source[key]
  warnings.push(`Unscoped per-registry settings (${dropped.join(', ')}) in "${sourceLabel}" were ignored: ` +
    `the source's "registry" value (${JSON.stringify(source.registry)}) is not a parseable URL, so pnpm cannot pin them anywhere safe. ` +
    'Write them URL-scoped (e.g. "//registry.example.com/:_authToken=...") to send them to a specific registry.')
}

function pinUnscopedCreds (source: Record<string, unknown>, nerfedRegistry: string): string[] {
  const rescoped: string[] = []
  for (const key of UNSCOPED_RESCOPABLE_KEYS) {
    if (!(key in source)) continue
    const scopedKey = `${nerfedRegistry}:${key}`
    if (!(scopedKey in source)) {
      source[scopedKey] = source[key]
    }
    delete source[key]
    rescoped.push(key)
  }
  return rescoped
}
