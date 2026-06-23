#!/usr/bin/env node
// Mirrors the Node.js release team's OpenPGP public keys (used to verify the
// signature of SHASUMS256.txt) from the canonical nodejs/release-keys repo into
// src/nodeReleaseKeys.ts and pacquet's matching Rust key module.
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
// The TypeScript pnpm CLI lives under pnpm11/, while pacquet stays at the repo root.
const ROOT = path.join(path.dirname(fileURLToPath(import.meta.url)), '..', '..', '..')
const REPO_ROOT = path.join(ROOT, '..')
const TS_KEYS_FILE = path.join(ROOT, 'crypto', 'shasums-file', 'src', 'nodeReleaseKeys.ts')
const RUST_KEYS_FILE = path.join(REPO_ROOT, 'pacquet', 'crates', 'crypto-shasums-file', 'src', 'node_release_keys.rs')

async function main () {
  const update = process.argv.includes('--update')
  const fingerprints = (await (await fetchOk(`${RAW}/keys.list`)).text())
    .split('\n').map((l) => l.trim()).filter(Boolean)

  const embedded = [
    readEmbeddedFingerprints(TS_KEYS_FILE, /fingerprint: '([0-9A-F]+)'/g),
    readEmbeddedFingerprints(RUST_KEYS_FILE, /fingerprint: "([0-9A-F]+)"/g),
  ]
  const missing = embedded.flatMap(({ label, fingerprints: embeddedFingerprints }) =>
    fingerprints.filter((fp) => !embeddedFingerprints.includes(fp)).map((fp) => ({ label, fp }))
  )
  // Keys embedded here but no longer in the canonical list (e.g. revoked/rotated)
  // must NOT stay in the trust set, so they fail the check too.
  const extra = embedded.flatMap(({ label, fingerprints: embeddedFingerprints }) =>
    embeddedFingerprints.filter((fp) => !fingerprints.includes(fp)).map((fp) => ({ label, fp }))
  )

  if (!update) {
    if (missing.length === 0 && extra.length === 0) {
      console.log(`✓ Embedded Node.js release keys are up to date (${fingerprints.length} key(s)).`)
      return
    }
    console.error('✗ Embedded Node.js release keys are out of sync with nodejs/release-keys.')
    if (missing.length > 0) console.error(`  Missing (add): ${formatDrift(missing)}`)
    if (extra.length > 0) console.error(`  No longer canonical (remove — possibly revoked): ${formatDrift(extra)}`)
    console.error(`Run: node ${path.relative(process.cwd(), fileURLToPath(import.meta.url))} --update`)
    process.exit(1)
  }

  const keys = []
  for (const fp of fingerprints) {
    const armored = (await (await fetchOk(`${RAW}/keys/${fp}.asc`)).text()).trim()
    keys.push({ fingerprint: fp, armored })
  }
  fs.writeFileSync(TS_KEYS_FILE, renderTypeScript(keys))
  fs.writeFileSync(RUST_KEYS_FILE, renderRust(keys))
  console.log(`✓ Wrote ${keys.length} Node.js release key(s).`)
}

async function fetchOk (url) {
  const res = await fetch(url)
  if (!res.ok) throw new Error(`Failed to fetch ${url}: ${res.status}`)
  return res
}

function readEmbeddedFingerprints (file, pattern) {
  const label = path.relative(REPO_ROOT, file)
  if (!fs.existsSync(file)) return { label, fingerprints: [] }
  return {
    label,
    fingerprints: [...fs.readFileSync(file, 'utf8').matchAll(pattern)].map((m) => m[1]),
  }
}

function formatDrift (items) {
  return items.map(({ label, fp }) => `${label}: ${fp}`).join(', ')
}

function renderTypeScript (keys) {
  const entries = keys.map(({ fingerprint, armored }) =>
    `  {\n    fingerprint: '${fingerprint}',\n    armoredKey: ${JSON.stringify(`${armored}\n`)},\n  },`).join('\n')
  return `/* eslint-disable */
// cspell:disable
// GENERATED — the Node.js release team's OpenPGP public keys, mirrored from
// <https://github.com/nodejs/release-keys> (keys.list + keys/<fingerprint>.asc).
//
// Used to verify the signature of a Node.js release's SHASUMS256.txt before
// trusting its hashes. Refresh with:
//   node crypto/shasums-file/scripts/update-node-release-keys.mjs --update
export const NODE_RELEASE_KEYS = [
${entries}
] as const satisfies ReadonlyArray<{ fingerprint: string, armoredKey: string }>
`
}

function renderRust (keys) {
  const entries = keys.map(({ fingerprint, armored }) =>
    `    NodeReleaseKey {\n        fingerprint: "${fingerprint}",\n        armored_key: r#"${armored}\n"#,\n    },`).join('\n')
  return `// GENERATED - the Node.js release team's OpenPGP public keys, mirrored from
// https://github.com/nodejs/release-keys (keys.list + keys/<fingerprint>.asc).
//
// Used to verify the signature of a Node.js release's SHASUMS256.txt before
// trusting its hashes. Refresh with:
//   node crypto/shasums-file/scripts/update-node-release-keys.mjs --update
pub(crate) struct NodeReleaseKey {
    pub(crate) fingerprint: &'static str,
    pub(crate) armored_key: &'static str,
}

pub(crate) const NODE_RELEASE_KEYS: &[NodeReleaseKey] = &[
${entries}
];
`
}

main().catch((err) => { console.error(err); process.exit(1) })
