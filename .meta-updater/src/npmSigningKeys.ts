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
