#!/usr/bin/env node
// Keeps the embedded npm registry signing keys (src/npmSigningKeys.ts) in sync
// with https://registry.npmjs.org/-/npm/v1/keys.
//
//   node update-npm-signing-keys.mjs            # check (CI / release gate)
//   node update-npm-signing-keys.mjs --update   # rewrite the embedded keys
//
// `--check` fails when npm advertises a signing key that is not embedded
// verbatim, so a key rotation cannot silently break (or weaken) pnpm's
// signature verification. `--update` writes the union of npm's keys and any
// embedded keys npm no longer lists (older keys are kept so packages published
// before a rotation still verify).
import fs from 'node:fs'
import path from 'node:path'
import { fileURLToPath } from 'node:url'

const KEYS_URL = 'https://registry.npmjs.org/-/npm/v1/keys'
const KEYS_FILE = path.join(path.dirname(fileURLToPath(import.meta.url)), '..', 'src', 'npmSigningKeys.ts')
const KEY_FIELDS = ['expires', 'keyid', 'keytype', 'scheme', 'key']

async function main () {
  const update = process.argv.includes('--update')
  const npmKeys = await fetchNpmKeys()
  const embedded = readEmbeddedKeys()

  const missing = npmKeys.filter((npmKey) => !embedded.some((e) => keysEqual(e, npmKey)))

  if (!update) {
    if (missing.length === 0) {
      console.log(`✓ Embedded npm signing keys are up to date (${embedded.length} key(s)).`)
      return
    }
    console.error(`✗ Embedded npm signing keys are out of date. ${missing.length} key(s) advertised by npm are not embedded:`)
    for (const key of missing) console.error(`  - ${key.keyid}`)
    console.error(`\nRun: node ${path.relative(process.cwd(), fileURLToPath(import.meta.url))} --update`)
    process.exit(1)
  }

  // Union: every npm key, plus embedded keys npm no longer lists (kept for
  // verifying packages published before a rotation).
  const merged = [...npmKeys]
  for (const e of embedded) {
    if (!merged.some((m) => m.keyid === e.keyid)) merged.push(e)
  }
  fs.writeFileSync(KEYS_FILE, render(merged))
  console.log(missing.length === 0
    ? '✓ Embedded npm signing keys already current; rewrote file.'
    : `✓ Updated embedded npm signing keys (added ${missing.length}).`)
}

async function fetchNpmKeys () {
  const res = await fetch(KEYS_URL)
  if (!res.ok) throw new Error(`Failed to fetch ${KEYS_URL}: ${res.status}`)
  const body = await res.json()
  if (!Array.isArray(body?.keys)) throw new Error(`Unexpected response from ${KEYS_URL}`)
  return body.keys.map(pickFields)
}

function readEmbeddedKeys () {
  const source = fs.readFileSync(KEYS_FILE, 'utf8')
  const start = source.indexOf('[', source.indexOf('NPM_SIGNING_KEYS'))
  if (start === -1) throw new Error(`Could not find NPM_SIGNING_KEYS array in ${KEYS_FILE}`)
  let depth = 0
  let end = -1
  for (let i = start; i < source.length; i++) {
    if (source[i] === '[') depth++
    else if (source[i] === ']' && --depth === 0) { end = i + 1; break }
  }
  if (end === -1) throw new Error(`Unterminated NPM_SIGNING_KEYS array in ${KEYS_FILE}`)
  return JSON.parse(source.slice(start, end)).map(pickFields)
}

function pickFields (key) {
  const out = {}
  for (const f of KEY_FIELDS) out[f] = key[f] ?? null
  return out
}

function keysEqual (a, b) {
  return KEY_FIELDS.every((f) => (a[f] ?? null) === (b[f] ?? null))
}

function render (keys) {
  const body = JSON.stringify(keys, KEY_FIELDS, 2)
  return `/* eslint-disable */
// GENERATED — npm's public registry signing keys, mirrored from
// ${KEYS_URL}
//
// Refresh with: node deps/security/signatures/scripts/update-npm-signing-keys.mjs --update
// The release workflow runs \`--check\` and fails if these drift from npm, so a
// rotated key cannot silently break (or weaken) signature verification.
export const NPM_SIGNING_KEYS = ${body} as const satisfies ReadonlyArray<{
  expires: string | null
  keyid: string
  keytype: string
  scheme: string
  key: string
}>
`
}

main().catch((err) => {
  console.error(err)
  process.exit(1)
})
