import assert from 'node:assert/strict'
import fs from 'node:fs'
import os from 'node:os'
import path from 'node:path'
import { test } from 'node:test'
import { checkTree, dataHome, keptDownload, rustupHome, sha256 } from './install-creusot.mjs'

function scratch (context) {
  const root = fs.realpathSync(fs.mkdtempSync(path.join(os.tmpdir(), 'pnpm-install-creusot-test-')))
  context.after(() => fs.rmSync(root, { recursive: true, force: true }))
  return root
}

test('finds the data directory the way cargo-creusot does', () => {
  assert.equal(dataHome({ CREUSOT_DATA_HOME: '/data', XDG_DATA_HOME: '/xdg' }, '/home'), '/data')
  assert.equal(dataHome({ XDG_DATA_HOME: '/xdg' }, '/home'), path.join('/xdg', 'creusot'))
  assert.equal(dataHome({}, '/home'), path.join('/home', '.local', 'share', 'creusot'))
})

test('finds the toolchains directory the way rustup does', () => {
  assert.equal(rustupHome({ RUSTUP_HOME: '/rustup' }, '/home'), '/rustup')
  assert.equal(rustupHome({}, '/home'), path.join('/home', '.rustup'))
})

const abcSha256 = 'ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad'

test('reuses a kept download only while it matches its pin', (context) => {
  const downloads = scratch(context)
  assert.equal(keptDownload(undefined, 'archive.tar.gz', abcSha256), undefined)
  assert.equal(keptDownload(downloads, 'archive.tar.gz', abcSha256), undefined)
  fs.writeFileSync(path.join(downloads, 'archive.tar.gz'), 'abc')
  assert.deepEqual(keptDownload(downloads, 'archive.tar.gz', abcSha256), Buffer.from('abc'))
  fs.writeFileSync(path.join(downloads, 'archive.tar.gz'), 'tampered')
  assert.equal(keptDownload(downloads, 'archive.tar.gz', abcSha256), undefined)
})

test('hashes bytes as hex SHA-256', () => {
  assert.equal(sha256(Buffer.from('abc')), abcSha256)
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
