import assert from 'node:assert/strict'
import { test } from 'node:test'
import { expectsProofs, provedFileCount, verifiedPackages } from './creusot.mjs'

function member (name, dependencies) {
  return { name, dependencies }
}

test('selects the members that take creusot-std under cfg(creusot), sorted', () => {
  const metadata = {
    packages: [
      member('pnpm-zeta', [{ name: 'creusot-std', target: 'cfg(creusot)' }]),
      member('pnpm-plain', [{ name: 'serde', target: null }]),
      member('pnpm-alpha', [{ name: 'pipe-trait', target: null }, { name: 'creusot-std', target: 'cfg(creusot)' }]),
    ],
  }
  assert.deepEqual(verifiedPackages(metadata), ['pnpm-alpha', 'pnpm-zeta'])
})

test('ignores creusot-std taken under any other condition', () => {
  const metadata = {
    packages: [
      member('pnpm-unconditional', [{ name: 'creusot-std', target: null }]),
      member('pnpm-other-target', [{ name: 'creusot-std', target: 'cfg(unix)' }]),
    ],
  }
  assert.deepEqual(verifiedPackages(metadata), [])
})

test('reads the number of proved files from the why3find summary', () => {
  assert.equal(provedFileCount('    Finished `dev` profile\nProved (23 files) ✔\n'), 23)
  assert.equal(provedFileCount('Proved (1 file) ✔\n'), 1)
})

test('counts no proved files when why3find found none to prove', () => {
  assert.equal(provedFileCount('Warning: No files to prove\nSummary:\nWarning: No files to prove\n'), 0)
})

test('expects proofs unless the arguments stop cargo creusot before proving', () => {
  assert.equal(expectsProofs([]), true)
  assert.equal(expectsProofs(['--no-cache']), true)
  assert.equal(expectsProofs(['--only', 'coma']), false)
  assert.equal(expectsProofs(['--only=coma']), false)
  assert.equal(expectsProofs(['--dry-run-why3find']), false)
})
