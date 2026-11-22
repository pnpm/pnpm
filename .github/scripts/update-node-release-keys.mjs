#!/usr/bin/env node
// Mirrors the Node.js release team's OpenPGP public keys (used to verify the
// signature of SHASUMS256.txt) from the canonical nodejs/release-keys repo into
// the Rust key module of pnpm-crypto-shasums-file.
//
//   node .github/scripts/update-node-release-keys.mjs            # check
//   node .github/scripts/update-node-release-keys.mjs --update   # rewrite the embedded keys
//
// The create-release-pr workflow runs `--update`, so any drift from the
// authoritative keys.list is committed into the release PR and reviewed there.
// Check mode reports the drift without writing.
import fs from 'node:fs'
import path from 'node:path'
import process from 'node:process'
import { fileURLToPath } from 'node:url'

const RAW = 'https://raw.githubusercontent.com/nodejs/release-keys/main'
const REPO_ROOT = path.join(path.dirname(fileURLToPath(import.meta.url)), '..', '..')
const KEYS_FILE = path.join(REPO_ROOT, 'pnpm', 'crates', 'crypto-shasums-file', 'src', 'node_release_keys.rs')

async function main () {
  const update = process.argv.includes('--update')
  const fingerprints = (await (await fetchOk(`${RAW}/keys.list`)).text())
    .split('\n').map((line) => line.trim()).filter(Boolean)

  const embedded = [...fs.readFileSync(KEYS_FILE, 'utf8').matchAll(/fingerprint: "([0-9A-F]+)"/g)].map((match) => match[1])
  const missing = fingerprints.filter((fp) => !embedded.includes(fp))
  // Keys embedded here but no longer in the canonical list (e.g. revoked/rotated)
  // must NOT stay in the trust set, so they fail the check too.
  const extra = embedded.filter((fp) => !fingerprints.includes(fp))

  if (!update) {
    if (missing.length === 0 && extra.length === 0) {
      console.log(`✓ Embedded Node.js release keys are up to date (${fingerprints.length} key(s)).`)
      return
    }
    console.error('✗ Embedded Node.js release keys are out of sync with nodejs/release-keys.')
    if (missing.length > 0) console.error(`  Missing (add): ${missing.join(', ')}`)
    if (extra.length > 0) console.error(`  No longer canonical (remove — possibly revoked): ${extra.join(', ')}`)
    console.error(`Run: node ${path.relative(process.cwd(), fileURLToPath(import.meta.url))} --update`)
    process.exit(1)
  }

  const keys = []
  for (const fp of fingerprints) {
    const armored = (await (await fetchOk(`${RAW}/keys/${fp}.asc`)).text()).trim()
    keys.push({ fingerprint: fp, armored })
  }
  writeKeyAssets(keys)
  fs.writeFileSync(KEYS_FILE, renderRust(keys))
  console.log(`✓ Wrote ${keys.length} Node.js release key(s).`)
}

async function fetchOk (url) {
  const res = await fetch(url)
  if (!res.ok) throw new Error(`Failed to fetch ${url}: ${res.status}`)
  return res
}

function writeKeyAssets (keys) {
  const directory = path.join(path.dirname(KEYS_FILE), 'node_release_keys')
  fs.mkdirSync(directory, { recursive: true })
  const files = new Set(keys.map(({ fingerprint }) => `${fingerprint}.asc`))
  for (const name of fs.readdirSync(directory)) {
    if (/^[0-9A-F]+\.asc$/.test(name) && !files.has(name)) {
      fs.unlinkSync(path.join(directory, name))
    }
  }
  for (const { fingerprint, armored } of keys) {
    fs.writeFileSync(path.join(directory, `${fingerprint}.asc`), `${armored}\n`)
  }
}

function renderRust (keys) {
  const entries = keys.map(({ fingerprint }) =>
    `    TrustedReleaseKey {\n        fingerprint: "${fingerprint}",\n        armored_key: include_str!("node_release_keys/${fingerprint}.asc"),\n    },`).join('\n')
  return `// GENERATED: the Node.js release team's OpenPGP public keys, mirrored from
// <https://github.com/nodejs/release-keys> (keys.list + keys/<fingerprint>.asc).
//
// Used to verify the signature of a Node.js release's SHASUMS256.txt before
// trusting its hashes. Refresh with:
//   node .github/scripts/update-node-release-keys.mjs --update
use crate::TrustedReleaseKey;

pub(crate) const NODE_RELEASE_KEYS: &[TrustedReleaseKey] = &[
${entries}
];
`
}

main().catch((err) => { console.error(err); process.exit(1) })
