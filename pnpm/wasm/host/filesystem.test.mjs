import assert from 'node:assert/strict'
import fs from 'node:fs'
import os from 'node:os'
import path from 'node:path'
import process from 'node:process'
import { test } from 'node:test'
import { TextEncoder } from 'node:util'
import { setTimeout } from 'node:timers/promises'

import { createWasiFilesystem } from './filesystem.mjs'
import { errno } from './wasi-errno.mjs'

const { WebAssembly } = globalThis

function fixture (context) {
  const directory = fs.mkdtempSync(path.join(os.tmpdir(), 'pnpm-wasm-fs-'))
  context.after(() => fs.rmSync(directory, { recursive: true, force: true }))
  const memory = new WebAssembly.Memory({ initial: 1, maximum: 2, shared: true })
  const { wasi, imports, acquireDescriptor } = createWasiFilesystem({ version: 'preview1', preopens: { '/': '/' } }, memory)
  wasi.initialize({ exports: { memory } })
  context.after(() => wasi.wasiImport.fd_close(3))
  function guestPath (file) {
    const bytes = new TextEncoder().encode(file)
    new Uint8Array(memory.buffer, 64, bytes.length).set(bytes)
    return [64, bytes.length]
  }
  return { directory, memory, wasi: wasi.wasiImport, imports, guestPath, acquireDescriptor }
}

test('private creation returns a WASI descriptor with private host mode before the first write', context => {
  const { directory, memory, wasi, imports, guestPath } = fixture(context)
  const file = path.join(directory, 'credential')
  assert.equal(imports.create_new(...guestPath(file), 0o600, 16), 0)
  const descriptor = new DataView(memory.buffer).getUint32(16, true)
  assert.equal(fs.statSync(file).mode & 0o777, 0o600)
  new Uint8Array(memory.buffer, 1024, 6).set(new TextEncoder().encode('secret'))
  const view = new DataView(memory.buffer)
  view.setUint32(32, 1024, true)
  view.setUint32(36, 6, true)
  assert.equal(wasi.fd_write(descriptor, 32, 1, 40), 0)
  assert.equal(fs.readFileSync(file, 'utf8'), 'secret')
  assert.equal(wasi.fd_close(descriptor), 0)
  assert.equal(imports.fchmod(descriptor, 0o777), 8)
})

test('exclusive creation rejects a symlink without changing its target', context => {
  const { directory, wasi, imports, guestPath } = fixture(context)
  const target = path.join(directory, 'target')
  const link = path.join(directory, 'link')
  fs.writeFileSync(target, 'keep', { mode: 0o600 })
  fs.symlinkSync(target, link)
  assert.equal(imports.create_new(...guestPath(link), 0o644, 16), 20)
  assert.equal(fs.readFileSync(target, 'utf8'), 'keep')
  assert.equal(fs.statSync(target).mode & 0o777, 0o600)
  assert.equal(imports.open_nofollow(...guestPath(link), 16), 32)
  assert.equal(wasi.fd_close(99), 8)
})

test('chmod stays bound to the opened file after its path is replaced', context => {
  const { directory, memory, wasi, imports, guestPath } = fixture(context)
  const file = path.join(directory, 'original')
  const moved = path.join(directory, 'moved')
  assert.equal(imports.create_new(...guestPath(file), 0o600, 16), 0)
  const descriptor = new DataView(memory.buffer).getUint32(16, true)
  fs.renameSync(file, moved)
  fs.writeFileSync(file, 'replacement', { mode: 0o600 })
  assert.equal(imports.fchmod(descriptor, 0o755), 0)
  assert.equal(fs.statSync(moved).mode & 0o777, 0o755)
  assert.equal(fs.statSync(file).mode & 0o777, 0o600)
  assert.equal(imports.fmode(descriptor, 20), 0)
  assert.equal(new DataView(memory.buffer).getUint32(20, true) & 0o777, 0o755)
  assert.equal(wasi.fd_close(descriptor), 0)
})

test('directory open rejects an intermediate symlink below its template', context => {
  const { directory, memory, wasi, imports, guestPath } = fixture(context)
  const template = path.join(directory, 'store')
  const outside = path.join(directory, 'outside')
  fs.mkdirSync(template)
  fs.mkdirSync(path.join(outside, 'child'), { recursive: true })
  fs.chmodSync(path.join(outside, 'child'), 0o700)
  fs.symlinkSync(outside, path.join(template, 'link'))
  const target = path.join(template, 'link', 'child')
  const templateBytes = new TextEncoder().encode(template)
  new Uint8Array(memory.buffer, 1024, templateBytes.length).set(templateBytes)
  assert.equal(imports.open_directory_nofollow_beneath(...guestPath(target), 1024, templateBytes.length, 16), 32)
  assert.equal(fs.statSync(path.join(outside, 'child')).mode & 0o777, 0o700)
  fs.mkdirSync(path.join(template, 'real'))
  assert.equal(imports.open_directory_nofollow_beneath(...guestPath(path.join(template, 'real')), 1024, templateBytes.length, 16), 0)
  const descriptor = new DataView(memory.buffer).getUint32(16, true)
  assert.equal(wasi.fd_close(descriptor), 0)
})

test('Linux directory grant stays bound to the opened template after a rename', { skip: process.platform !== 'linux' || 'webcontainer' in process.versions }, context => {
  const { directory, memory, imports, guestPath } = fixture(context)
  const template = path.join(directory, 'template')
  const moved = path.join(directory, 'moved')
  const outside = path.join(directory, 'outside')
  fs.mkdirSync(path.join(template, 'child'), { recursive: true })
  fs.mkdirSync(path.join(outside, 'child'), { recursive: true })
  fs.chmodSync(path.join(template, 'child'), 0o300)
  fs.chmodSync(path.join(outside, 'child'), 0o300)
  const templateBytes = new TextEncoder().encode(template)
  new Uint8Array(memory.buffer, 1024, templateBytes.length).set(templateBytes)
  const originalOpen = fs.openSync
  let movedTemplate = false
  fs.openSync = (file, ...args) => {
    const descriptor = originalOpen(file, ...args)
    if (file === template && !movedTemplate) {
      movedTemplate = true
      fs.renameSync(template, moved)
      fs.symlinkSync(outside, template)
    }
    return descriptor
  }
  try {
    assert.equal(imports.grant_directory_mode_beneath(...guestPath(path.join(template, 'child')), 1024, templateBytes.length, 0o2070), 0)
  } finally {
    fs.openSync = originalOpen
  }
  assert.equal(fs.statSync(path.join(moved, 'child')).mode & 0o7777, 0o2370)
  assert.equal(fs.statSync(path.join(outside, 'child')).mode & 0o7777, 0o300)
  fs.chmodSync(path.join(moved, 'child'), 0o700)
  fs.chmodSync(path.join(outside, 'child'), 0o700)
})

test('directory grant selects the WASI fallback without procfd support', context => {
  const { directory, memory, wasi, imports, guestPath } = fixture(context)
  const template = path.join(directory, 'template')
  const child = path.join(template, 'child')
  fs.mkdirSync(child, { recursive: true })
  const templateBytes = new TextEncoder().encode(template)
  new Uint8Array(memory.buffer, 1024, templateBytes.length).set(templateBytes)
  const originalExists = fs.existsSync
  fs.existsSync = file => file === '/proc/self/fd' ? false : originalExists(file)
  try {
    assert.equal(imports.grant_directory_mode_beneath(...guestPath(child), 1024, templateBytes.length, 0o2070), errno.ENOTSUP)
  } finally {
    fs.existsSync = originalExists
  }
  assert.equal(imports.open_directory_nofollow_beneath(...guestPath(child), 1024, templateBytes.length, 16), 0)
  assert.equal(wasi.fd_close(new DataView(memory.buffer).getUint32(16, true)), 0)
})

test('ordinary WASI opens join the same descriptor mapping and follow relative symlinks', context => {
  const { directory, memory, wasi, imports, guestPath } = fixture(context)
  const target = path.join(directory, 'target')
  fs.writeFileSync(target, 'data', { mode: 0o600 })
  const link = path.join(directory, 'link')
  fs.symlinkSync('target', link)
  assert.equal(wasi.path_open(3, 1, ...guestPath(link), 0, 2n | 2097152n, 0n, 0, 16), 0)
  const descriptor = new DataView(memory.buffer).getUint32(16, true)
  assert.equal(imports.fchmod(descriptor, 0o700), 0)
  assert.equal(fs.statSync(target).mode & 0o777, 0o700)
  assert.equal(wasi.fd_close(descriptor), 0)
})

test('descriptor renumbering moves permission operations to the destination', context => {
  const { directory, memory, wasi, imports, guestPath } = fixture(context)
  assert.equal(imports.create_new(...guestPath(path.join(directory, 'source')), 0o600, 16), 0)
  const source = new DataView(memory.buffer).getUint32(16, true)
  assert.equal(imports.create_new(...guestPath(path.join(directory, 'destination')), 0o600, 16), 0)
  const destination = new DataView(memory.buffer).getUint32(16, true)
  assert.equal(wasi.fd_renumber(source, destination), 0)
  assert.equal(imports.fchmod(source, 0o700), 8)
  assert.equal(imports.fchmod(destination, 0o700), 0)
  assert.equal(fs.statSync(path.join(directory, 'source')).mode & 0o777, 0o700)
  assert.equal(fs.statSync(path.join(directory, 'destination')).mode & 0o777, 0o600)
  assert.equal(wasi.fd_close(destination), 0)
})

test('filesystem extensions reject a remapped root', context => {
  const directory = fs.mkdtempSync(path.join(os.tmpdir(), 'pnpm-wasm-confined-'))
  context.after(() => fs.rmSync(directory, { recursive: true, force: true }))
  const memory = new WebAssembly.Memory({ initial: 1 })
  const { wasi, imports, close } = createWasiFilesystem({ version: 'preview1', preopens: { '/': directory } }, memory)
  wasi.initialize({ exports: { memory } })
  context.after(close)
  const bytes = new TextEncoder().encode('/../../outside')
  new Uint8Array(memory.buffer, 64, bytes.length).set(bytes)
  assert.equal(imports.create_new(64, bytes.length, 0o600, 16), 2)
  assert.equal(imports.open_nofollow(64, bytes.length, 16), 2)
  assert.equal(imports.lmode(64, bytes.length, 16), 2)
})

test('secure lock directory rejects a substituted symlink', context => {
  const { directory, imports, guestPath } = fixture(context)
  const target = path.join(directory, 'target')
  const lock = path.join(directory, 'lock')
  fs.mkdirSync(target, { mode: 0o755 })
  fs.symlinkSync(target, lock)
  assert.equal(imports.secure_directory(...guestPath(lock)), 54)
  assert.equal(fs.statSync(target).mode & 0o777, 0o755)
  fs.unlinkSync(lock)
  assert.equal(imports.secure_directory(...guestPath(lock)), 0)
  assert.equal(fs.statSync(lock).mode & 0o777, 0o700)
})

test('WASI creates an absolute symlink under the full host root preopen', context => {
  const { directory, memory, wasi } = fixture(context)
  const target = path.join(directory, 'missing-target')
  const link = path.join(directory, 'absolute-link')
  const targetBytes = new TextEncoder().encode(target)
  const linkBytes = new TextEncoder().encode(link)
  new Uint8Array(memory.buffer, 64, targetBytes.length).set(targetBytes)
  new Uint8Array(memory.buffer, 1024, linkBytes.length).set(linkBytes)
  assert.equal(wasi.path_symlink(64, targetBytes.length, 3, 1024, linkBytes.length), 0)
  assert.equal(fs.readlinkSync(link), target)
})


test('closing a WASI descriptor releases its file lease', async context => {
  const { directory, memory, wasi, imports, guestPath } = fixture(context)
  const file = path.join(directory, 'lease')
  assert.equal(imports.create_new(...guestPath(file), 0o600, 16), 0)
  const source = new DataView(memory.buffer).getUint32(16, true)
  assert.equal(imports.open_lock(...guestPath(file), 16), 0)
  const other = new DataView(memory.buffer).getUint32(16, true)
  const deadline = Date.now() + 5000
  while (imports.try_lock(source, 1) !== 0) {
    assert.ok(Date.now() < deadline)
    // eslint-disable-next-line no-await-in-loop -- Poll until the previous lock attempt completes.
    await setTimeout(5)
  }
  assert.equal(imports.try_lock(other, 1), 6)
  assert.equal(wasi.fd_close(source), 0)
  while (imports.try_lock(other, 1) !== 0) {
    assert.ok(Date.now() < deadline)
    // eslint-disable-next-line no-await-in-loop -- Poll until the previous lock attempt completes.
    await setTimeout(5)
  }
  assert.equal(wasi.fd_close(other), 0)
})


test('host descriptor leases survive guest close until every consumer releases', context => {
  const { directory, memory, wasi, imports, guestPath, acquireDescriptor } = fixture(context)
  const file = path.join(directory, 'leased')
  assert.equal(imports.create_new(...guestPath(file), 0o600, 16), 0)
  const descriptor = new DataView(memory.buffer).getUint32(16, true)
  const first = acquireDescriptor(descriptor)
  const second = acquireDescriptor(descriptor)
  assert.equal(wasi.fd_close(descriptor), 0)
  assert.throws(() => acquireDescriptor(descriptor), { code: 'EBADF' })
  first.release()
  first.release()
  fs.writeSync(second.fd, 'output after guest close')
  assert.equal(fs.readFileSync(file, 'utf8'), 'output after guest close')
  second.release()
  assert.throws(() => fs.fstatSync(second.fd), { code: 'EBADF' })
})


test('cross-user store ownership is rejected before recovery', { skip: process.getuid() === 0 }, context => {
  const { memory, wasi, imports, guestPath } = fixture(context)
  assert.equal(imports.open_nofollow(...guestPath('/etc/passwd'), 16), 0)
  const descriptor = new DataView(memory.buffer).getUint32(16, true)
  assert.equal(imports.check_owner(descriptor), 2)
  assert.equal(wasi.fd_close(descriptor), 0)
})

test('SQLite sidecars receive the private database mode at creation', context => {
  const { directory, memory, wasi, imports, guestPath } = fixture(context)
  const database = path.join(directory, 'index.db')
  fs.writeFileSync(database, 'private metadata', { mode: 0o600 })
  assert.equal(imports.sqlite_register(...guestPath(database), 1), 0)
  for (const suffix of ['-journal', '-wal', '-shm']) {
    const file = database + suffix
    assert.equal(wasi.path_open(3, 0, ...guestPath(file), 5, 64n | 2097152n, 0n, 0, 16), 0)
    assert.equal(fs.statSync(file).mode & 0o777, 0o600)
    assert.equal(wasi.fd_close(new DataView(memory.buffer).getUint32(16, true)), 0)
  }
  assert.equal(imports.sqlite_register(...guestPath(database), 0), 0)
})

test('random device reads never pass shared memory to host WebCrypto', context => {
  const { memory, wasi, guestPath } = fixture(context)
  assert.equal(wasi.path_open(3, 1, ...guestPath('/dev/urandom'), 0, 2n | 2097152n, 0n, 0, 16), 0)
  const view = new DataView(memory.buffer)
  const descriptor = view.getUint32(16, true)
  const read = fs.readSync
  context.mock.method(fs, 'readSync', (native, buffer, ...args) => {
    assert.equal(buffer.buffer instanceof globalThis.SharedArrayBuffer, false)
    return read(native, buffer, ...args)
  })
  view.setUint32(32, 1024, true)
  view.setUint32(36, 32, true)
  assert.equal(wasi.fd_read(descriptor, 32, 1, 40), 0)
  assert.equal(view.getUint32(40, true), 32)
  assert.ok(new Uint8Array(memory.buffer, 1024, 32).some(byte => byte !== 0))
  assert.equal(wasi.fd_close(descriptor), 0)
})

test('executable access follows the host file permission check', context => {
  const { directory, imports, guestPath } = fixture(context)
  const file = path.join(directory, 'command')
  fs.writeFileSync(file, 'command', { mode: 0o600 })
  assert.equal(imports.path_access_executable(...guestPath(file)), 2)
  context.mock.method(fs, 'accessSync', () => {})
  assert.equal(imports.path_access_executable(...guestPath(file)), 2)
  fs.chmodSync(file, 0o700)
  assert.equal(imports.path_access_executable(...guestPath(file)), 0)
  assert.equal(imports.path_access_executable(...guestPath(path.join(directory, 'missing'))), 44)
})

test('WASI resolves nested parent segments in absolute and preopen-relative paths', context => {
  const { directory, wasi, guestPath } = fixture(context)
  fs.mkdirSync(path.join(directory, 'packages', 'local'), { recursive: true })
  for (const absolute of [true, false]) {
    const name = absolute ? 'absolute-pack' : 'relative-pack'
    const destination = `${directory}/packages/local/../../${name}`
    assert.equal(wasi.path_create_directory(3, ...guestPath(absolute ? destination : destination.slice(1))), 0)
    assert.ok(fs.statSync(path.join(directory, name)).isDirectory())
    assert.equal(wasi.path_remove_directory(3, ...guestPath(`${directory}/packages/../${name}`)), 0)
    assert.equal(fs.existsSync(path.join(directory, name)), false)
  }
})

test('WASI preserves relative symlink targets with parent segments', context => {
  const { directory, memory, wasi, guestPath } = fixture(context)
  const nested = path.join(directory, 'packages', 'local')
  fs.mkdirSync(nested, { recursive: true })
  fs.writeFileSync(path.join(directory, 'source'), 'linked bytes')
  const target = '../../source'
  const targetBytes = new TextEncoder().encode(target)
  new Uint8Array(memory.buffer, 1024, targetBytes.length).set(targetBytes)
  const link = `${directory}/packages/local/../local/link`
  assert.equal(wasi.path_symlink(1024, targetBytes.length, 3, ...guestPath(link)), 0)
  assert.equal(fs.readlinkSync(path.join(nested, 'link')), target)
  assert.equal(fs.readFileSync(path.join(nested, 'link'), 'utf8'), 'linked bytes')
  assert.equal(wasi.path_open(3, 1, ...guestPath(link), 0, 2n | 2097152n, 0n, 0, 16), 0)
  assert.equal(wasi.fd_close(new DataView(memory.buffer).getUint32(16, true)), 0)
})
