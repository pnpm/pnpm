#!/usr/bin/env node
// Mirrors the Node.js release team's OpenPGP public keys (used to verify the
// signature of SHASUMS256.txt) from the canonical nodejs/release-keys repo into
// src/nodeReleaseKeys.ts.
//
//   node update-node-release-keys.mjs            # check (CI / release gate)
//   node update-node-release-keys.mjs --update   # rewrite the embedded keys
//
// `--check` fails when the authoritative keys.list contains a fingerprint that
// is not embedded, so a newly added release signer cannot silently break Node
// runtime verification.
import fs from 'node:fs'
import path from 'node:path'
import { fileURLToPath } from 'node:url'

const RAW = 'https://raw.githubusercontent.com/nodejs/release-keys/main'
const KEYS_FILE = path.join(path.dirname(fileURLToPath(import.meta.url)), '..', 'src', 'nodeReleaseKeys.ts')

async function main () {
  const update = process.argv.includes('--update')
  const fingerprints = (await (await fetchOk(`${RAW}/keys.list`)).text())
    .split('\n').map((l) => l.trim()).filter(Boolean)

  const embedded = readEmbeddedFingerprints()
  const missing = fingerprints.filter((fp) => !embedded.includes(fp))

  if (!update) {
    if (missing.length === 0) {
      console.log(`✓ Embedded Node.js release keys are up to date (${embedded.length} key(s)).`)
      return
    }
    console.error(`✗ Embedded Node.js release keys are out of date. Missing: ${missing.join(', ')}`)
    console.error(`Run: node ${path.relative(process.cwd(), fileURLToPath(import.meta.url))} --update`)
    process.exit(1)
  }

  const keys = []
  for (const fp of fingerprints) {
    const armored = (await (await fetchOk(`${RAW}/keys/${fp}.asc`)).text()).trim()
    keys.push({ fingerprint: fp, armored })
  }
  fs.writeFileSync(KEYS_FILE, render(keys))
  console.log(`✓ Wrote ${keys.length} Node.js release key(s).`)
}

async function fetchOk (url) {
  const res = await fetch(url)
  if (!res.ok) throw new Error(`Failed to fetch ${url}: ${res.status}`)
  return res
}

function readEmbeddedFingerprints () {
  if (!fs.existsSync(KEYS_FILE)) return []
  return [...fs.readFileSync(KEYS_FILE, 'utf8').matchAll(/fingerprint: '([0-9A-F]+)'/g)].map((m) => m[1])
}

function render (keys) {
  const entries = keys.map(({ fingerprint, armored }) =>
    `  {\n    fingerprint: '${fingerprint}',\n    armoredKey: ${JSON.stringify(`${armored}\n`)},\n  },`).join('\n')
  return `/* eslint-disable */
// cspell:disable
// GENERATED — the Node.js release team's OpenPGP public keys, mirrored from
// https://github.com/nodejs/release-keys (keys.list + keys/<fingerprint>.asc).
//
// Used to verify the signature of a Node.js release's SHASUMS256.txt before
// trusting its hashes. Refresh with:
//   node crypto/shasums-file/scripts/update-node-release-keys.mjs --update
export const NODE_RELEASE_KEYS = [
${entries}
] as const satisfies ReadonlyArray<{ fingerprint: string, armoredKey: string }>
`
}

main().catch((err) => { console.error(err); process.exit(1) })
