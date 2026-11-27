import assert from 'node:assert/strict'
import fs from 'node:fs'
import os from 'node:os'
import path from 'node:path'
import { test } from 'node:test'
import { checkTree, dataHome, parseRelease, sha256 } from './install-creusot.mjs'

function scratch (context) {
  const root = fs.realpathSync(fs.mkdtempSync(path.join(os.tmpdir(), 'pnpm-install-creusot-test-')))
  context.after(() => fs.rmSync(root, { recursive: true, force: true }))
  return root
}

const release = {
  rust_channel: 'nightly-2026-08-03',
  why3_prefix: '/opt/creusot-why3/_opam',
  assets: [{ name: 'why3', file: 'why3.tar.gz', prefix: 'why3-prefix', sha256: 'ab' }],
}

test('parses the fields the installer reads', () => {
  assert.deepEqual(parseRelease(JSON.stringify(release)), release)
})

test('rejects a release.json without a field the installer reads', () => {
  assert.throws(() => parseRelease(JSON.stringify({ ...release, why3_prefix: undefined })), /why3_prefix/)
  assert.throws(() => parseRelease(JSON.stringify({ ...release, assets: [] })), /no assets/)
  const asset = { ...release.assets[0], sha256: undefined }
  assert.throws(() => parseRelease(JSON.stringify({ ...release, assets: [asset] })), /sha256/)
})

test('finds the data directory the way cargo-creusot does', () => {
  assert.equal(dataHome({ CREUSOT_DATA_HOME: '/data', XDG_DATA_HOME: '/xdg' }, '/home'), '/data')
  assert.equal(dataHome({ XDG_DATA_HOME: '/xdg' }, '/home'), path.join('/xdg', 'creusot'))
  assert.equal(dataHome({}, '/home'), path.join('/home', '.local', 'share', 'creusot'))
})

test('hashes bytes as hex SHA-256', () => {
  assert.equal(sha256(Buffer.from('abc')), 'ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad')
})

test('accepts files, directories, and symlinks that stay in the archive', (context) => {
  const root = scratch(context)
  fs.mkdirSync(path.join(root, 'bin'))
  fs.writeFileSync(path.join(root, 'bin', 'why3'), '')
  fs.symlinkSync('why3', path.join(root, 'bin', 'why3-link'))
  checkTree(root)
})

test('rejects a symlink that leaves the archive', (context) => {
  const root = scratch(context)
  fs.symlinkSync('../outside', path.join(root, 'escape'))
  assert.throws(() => checkTree(root), /points outside the archive/)
})

test('rejects a symlink with an absolute target', (context) => {
  const root = scratch(context)
  fs.symlinkSync('/etc/passwd', path.join(root, 'absolute'))
  assert.throws(() => checkTree(root), /points outside the archive/)
})
