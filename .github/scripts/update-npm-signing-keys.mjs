#!/usr/bin/env node
// Keeps .config/npm-signing-keys.json, the npm registry signing keys that every
// CLI and installer in this repository pins, in sync with
// https://registry.npmjs.org/-/npm/v1/keys.
//
//   node .github/scripts/update-npm-signing-keys.mjs            # check, non-zero on drift
//   node .github/scripts/update-npm-signing-keys.mjs --update   # rewrite the file
//
// The meta-updater renders the file into the copy each product embeds, so
// `pnpm update-manifests` follows an update. pnpm/get.pnpm.io's install
// scripts read the same file.
//
// A key npm stops listing stays in the file, marked `"withdrawn": true`. pnpm
// keeps trusting it, so a package signed before a rotation still verifies.
// get-pnpm and the install scripts drop it: they only download current
// releases, and a signature carries no trusted timestamp that would tell one
// made before a withdrawal from one forged after it.
import fs from 'node:fs'
import path from 'node:path'
import process from 'node:process'
import { fileURLToPath } from 'node:url'

const KEYS_URL = 'https://registry.npmjs.org/-/npm/v1/keys'
export const KEYS_FILE = path.resolve(fileURLToPath(import.meta.url), '../../../.config/npm-signing-keys.json')
const KEY_FIELDS = ['expires', 'keyid', 'keytype', 'scheme', 'key']

/**
 * Every key npm lists, then every pinned key it no longer lists, marked
 * withdrawn. Sorted by key ID, so a reordering of npm's response changes
 * nothing.
 */
export function mergeKeys (pinned, advertised) {
  const merged = advertised.map(pickFields)
  for (const key of pinned) {
    if (!merged.some(({ keyid }) => keyid === key.keyid)) {
      merged.push({ ...pickFields(key), withdrawn: true })
    }
  }
  return merged.sort((a, b) => a.keyid.localeCompare(b.keyid))
}

export function renderKeys (keys) {
  return `${JSON.stringify(keys, [...KEY_FIELDS, 'withdrawn'], 2)}\n`
}

function pickFields (key) {
  return Object.fromEntries(KEY_FIELDS.map((field) => [field, key[field] ?? null]))
}

async function main () {
  const update = process.argv.includes('--update')
  const current = fs.readFileSync(KEYS_FILE, 'utf8')
  const expected = renderKeys(mergeKeys(JSON.parse(current), await fetchNpmKeys()))
  if (current === expected) {
    console.log(`✓ ${path.basename(KEYS_FILE)} matches the keys npm advertises.`)
    return
  }
  if (update) {
    fs.writeFileSync(KEYS_FILE, expected)
    console.log(`✓ Updated ${path.basename(KEYS_FILE)}. Run \`pnpm update-manifests\` to render it into the embedded copies.`)
    return
  }
  console.error(`✗ ${path.basename(KEYS_FILE)} does not match the keys npm advertises.`)
  console.error(`\nRun: node ${path.relative(process.cwd(), fileURLToPath(import.meta.url))} --update`)
  process.exitCode = 1
}

async function fetchNpmKeys () {
  const res = await fetch(KEYS_URL)
  if (!res.ok) throw new Error(`Failed to fetch ${KEYS_URL}: ${res.status}`)
  const body = await res.json()
  if (!Array.isArray(body?.keys)) throw new Error(`Unexpected response from ${KEYS_URL}`)
  return body.keys
}

if (process.argv[1] === fileURLToPath(import.meta.url)) {
  main().catch((err) => {
    console.error(err)
    process.exitCode = 1
  })
}
