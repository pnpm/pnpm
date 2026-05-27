import loadNpmConf from '@pnpm/npm-conf'
import { nerfDart } from '@pnpm/config.nerf-dart'
import normalizeRegistryUrl from 'normalize-registry-url'

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

const npmDefaults = loadNpmConf.defaults

// Rewrite any unscoped per-registry keys in `source` to their URL-scoped
// equivalents (`//host[:port]/path/:<key>=...`) using `source.registry` —
// or the builtin default registry if the source doesn't declare its own.
// This pins each layer's credential or client certificate to the registry
// that layer named (or the implicit npmjs default), so a later layer
// overriding `registry=` cannot pull a setting authored for one host
// along to a different host. A URL-scoped key for the same registry
// already present in `source` wins; we never overwrite an explicit scoped
// value.
//
// Each rewrite triggers a deprecation warning so users migrate to writing
// the URL-scoped form directly. npm has rejected unscoped credentials
// outright since `npm@9` (`ERR_INVALID_AUTH`).
export function rescopeUnscopedCreds (
  source: Record<string, unknown>,
  sourceLabel: string,
  warnings: string[]
): Record<string, unknown> {
  // npm-conf is built on config-chain/proto-list, which chains each source's
  // data object via prototype: list[0].__proto__ === list[1], and so on. So
  // `key in source` walks UP into other sources, and `source[key]` reads an
  // inherited value. We only ever want to rescope keys this source actually
  // declared — hence `Object.hasOwn` everywhere instead of `in`/dot access.
  //
  // `null`/`undefined` values come from npm-conf's defaults layer (it pre-
  // fills `cert: null`, `key: null`, etc.). They're not real settings and
  // must be skipped, otherwise we'd create URL-scoped null entries that
  // downstream consumers would choke on.
  if (!UNSCOPED_RESCOPABLE_KEYS.some(key => Object.hasOwn(source, key) && source[key] != null)) {
    return source
  }
  // Read the source's OWN registry — `source.registry` would walk the
  // prototype chain into a different source and pin our credentials there.
  const ownRegistry = Object.hasOwn(source, 'registry') ? source.registry : undefined
  const rawRegistry = typeof ownRegistry === 'string' && ownRegistry !== '' ? ownRegistry : null
  const fallbackRegistry = rawRegistry ?? npmDefaults.registry
  let nerfedRegistry: string
  try {
    nerfedRegistry = nerfDart(normalizeRegistryUrl(fallbackRegistry))
  } catch {
    // `registry=` resolved to something `URL` can't parse — often an
    // unresolved `${VAR}` placeholder that left the string empty. Drop the
    // unscoped keys (a bare token is unsafe to bind anywhere) and warn.
    const dropped = UNSCOPED_RESCOPABLE_KEYS.filter(key => Object.hasOwn(source, key) && source[key] != null)
    for (const key of dropped) delete source[key]
    warnings.push(`Unscoped per-registry settings (${dropped.join(', ')}) in "${sourceLabel}" were ignored: ` +
      `the source's "registry" value (${JSON.stringify(ownRegistry)}) is not a parseable URL, so pnpm cannot pin them anywhere safe. ` +
      'Write them URL-scoped (e.g. "//registry.example.com/:_authToken=...") to send them to a specific registry.')
    return source
  }
  const rescoped: string[] = []
  for (const key of UNSCOPED_RESCOPABLE_KEYS) {
    if (!Object.hasOwn(source, key) || source[key] == null) continue
    const scopedKey = `${nerfedRegistry}:${key}`
    if (!Object.hasOwn(source, scopedKey)) {
      source[scopedKey] = source[key]
    }
    delete source[key]
    rescoped.push(key)
  }
  if (rescoped.length > 0) {
    warnings.push(`Unscoped per-registry settings (${rescoped.join(', ')}) in "${sourceLabel}" are deprecated. ` +
      `pnpm pinned them to "${nerfedRegistry}" for this run, but a future release will stop supporting unscoped per-registry settings. ` +
      `Write them as "${nerfedRegistry}:${rescoped[0]}=..." instead.`)
  }
  return source
}
