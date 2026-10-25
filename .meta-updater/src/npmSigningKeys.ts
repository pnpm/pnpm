// Renders .config/npm-signing-keys.json into the copy of npm's registry signing
// keys that each product embeds. .github/scripts/update-npm-signing-keys.mjs
// keeps the JSON in sync with npm, and explains `withdrawn`.

export const NPM_SIGNING_KEYS_FILE = '.config/npm-signing-keys.json'

export interface NpmSigningKey {
  expires: string | null
  keyid: string
  keytype: string
  scheme: string
  key: string
  withdrawn?: true
}

const TS_KEY_FIELDS = ['expires', 'keyid', 'keytype', 'scheme', 'key']

/**
 * A TypeScript module exporting `NPM_SIGNING_KEYS`. `currentOnly` leaves out
 * the keys npm withdrew.
 */
export function renderNpmSigningKeysModule (keys: NpmSigningKey[], opts: { currentOnly: boolean }): string {
  const embedded = opts.currentOnly ? keys.filter((key) => key.withdrawn !== true) : keys
  return `/* eslint-disable */
// GENERATED from ${NPM_SIGNING_KEYS_FILE} by the meta-updater. Refresh the
// keys with \`pnpm update:npm-signing-keys\`, then \`pnpm update-manifests\`.
export const NPM_SIGNING_KEYS = ${JSON.stringify(embedded, TS_KEY_FIELDS, 2)} as const satisfies ReadonlyArray<{
  expires: string | null
  keyid: string
  keytype: string
  scheme: string
  key: string
}>
`
}

const RUST_KEYS_START = 'pub(super) const NPM_SIGNING_KEYS: &[NpmSigningKey] = &[\n'

/** `content` with the entries of its `NPM_SIGNING_KEYS` slice replaced by every key in `keys`. */
export function replaceRustNpmSigningKeys (content: string | null, keys: NpmSigningKey[]): string | null {
  if (content == null) return content
  const start = content.indexOf(RUST_KEYS_START)
  const end = content.indexOf('\n];\n', start)
  if (start === -1 || end === -1) {
    throw new Error('The NPM_SIGNING_KEYS slice was not found')
  }
  const entries = keys.map((key) => `    NpmSigningKey {
        keyid: ${rustString(key.keyid)},
        key: ${rustString(key.key)},
        expires: ${key.expires == null ? 'None' : `Some(${rustString(key.expires)})`},
    },`).join('\n')
  return `${content.slice(0, start + RUST_KEYS_START.length)}${entries}${content.slice(end)}`
}

/** A Rust string literal. Key IDs, keys, and dates need no escaping, so anything else is refused. */
function rustString (value: string): string {
  if (!/^[\w+/=:.-]*$/.test(value)) {
    throw new Error(`Unexpected character in an npm signing key field: ${value}`)
  }
  return `"${value}"`
}
