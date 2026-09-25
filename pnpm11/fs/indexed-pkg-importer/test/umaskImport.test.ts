import fs from 'node:fs'
import path from 'node:path'

import { expect, test } from '@jest/globals'
import type { PackageImportMethod } from '@pnpm/fs.indexed-pkg-importer'
import { createIndexedPkgImporter } from '@pnpm/fs.indexed-pkg-importer'
import { tempDir } from '@pnpm/prepare'

const testOnLinuxOnly = (process.platform === 'darwin' || process.platform === 'win32') ? test.skip : test

function desiredMode (executable: boolean): number {
  return ((executable ? 0o755 : 0o666) & ~process.umask()) & 0o777
}

function digest (seed: string): string {
  return seed.repeat(64)
}
const EXEC_CONTENT = Buffer.from('#!/usr/bin/env node\n')
const PLAIN_CONTENT = Buffer.from('module.exports = 1\n')
const PKG_JSON_CONTENT = Buffer.from('{"name":"pkg"}\n')

interface InstalledPackage {
  execBin: string
  plain: string
  packageJson: string
}

// FilesMap values for store imports point at files/{xx}/{hash}[-exec] inside
// the store, which is what lets the importer tell store sources from local ones.
function packageFromStore (method: PackageImportMethod, entries: { exec?: string, plain?: string }): InstalledPackage {
  const storeDir = path.join(tempDir(), 'store')
  const target = path.join(tempDir(), 'project/package')
  const filesMap = new Map<string, string>()
  const execBin = path.join(target, 'bin/cli.js')
  if (entries.exec) {
    addStoreEntry(storeDir, filesMap, 'bin/cli.js', entries.exec, true, (desiredMode(true) ^ 0b1) & 0o777, EXEC_CONTENT)
  }
  const plain = path.join(target, 'index.js')
  if (entries.plain) {
    addStoreEntry(storeDir, filesMap, 'index.js', entries.plain, false, (desiredMode(false) ^ 0b1) & 0o777, PLAIN_CONTENT)
  }
  addStoreEntry(storeDir, filesMap, 'package.json', 'd', false, (desiredMode(false) ^ 0b1) & 0o777, PKG_JSON_CONTENT)
  importPackage(method, target, filesMap)
  return { execBin, plain, packageJson: path.join(target, 'package.json') }
}

function importPackage (method: PackageImportMethod, target: string, filesMap: Map<string, string>): string | undefined {
  const result = createIndexedPkgImporter(method)(target, {
    filesMap,
    force: false,
    resolvedFrom: 'remote',
  })
  expect(result).toBe(method)
  return result
}

function mode (file: string): number {
  return fs.statSync(file).mode & 0o777
}

function inode (file: string): number {
  return fs.statSync(file).ino
}

testOnLinuxOnly('hardlink tier shares the store inode when the entry mode matches the current umask', () => {
  const storeDir = path.join(tempDir(), 'store')
  const execMode = desiredMode(true)
  const plainMode = desiredMode(false)
  const filesMap = new Map<string, string>()
  const execSrc = addStoreEntry(storeDir, filesMap, 'bin/cli.js', 'a', true, execMode, EXEC_CONTENT)
  const plainSrc = addStoreEntry(storeDir, filesMap, 'index.js', 'b', false, plainMode, PLAIN_CONTENT)
  addStoreEntry(storeDir, filesMap, 'package.json', 'c', false, plainMode, PKG_JSON_CONTENT)
  const target = path.join(tempDir(), 'project/package')

  expect(createIndexedPkgImporter('hardlink')(target, {
    filesMap,
    force: false,
    resolvedFrom: 'remote',
  })).toBe('hardlink')

  const execDest = path.join(target, 'bin/cli.js')
  const plainDest = path.join(target, 'index.js')
  expect(mode(execDest)).toBe(execMode)
  expect(mode(plainDest)).toBe(plainMode)
  expect(inode(execDest)).toBe(inode(execSrc))
  expect(inode(plainDest)).toBe(inode(plainSrc))
})

testOnLinuxOnly('hardlink tier copies at the current umask mode when the store entry mode differs', () => {
  const storeDir = path.join(tempDir(), 'store')
  const execMode = desiredMode(true)
  const plainMode = desiredMode(false)
  const filesMap = new Map<string, string>()
  const execSrc = addStoreEntry(storeDir, filesMap, 'bin/cli.js', 'a', true, execMode ^ 0b1, EXEC_CONTENT)
  addStoreEntry(storeDir, filesMap, 'index.js', 'b', false, plainMode ^ 0b1, PLAIN_CONTENT)
  addStoreEntry(storeDir, filesMap, 'package.json', 'c', false, plainMode ^ 0b1, PKG_JSON_CONTENT)
  const target = path.join(tempDir(), 'project/package')

  expect(createIndexedPkgImporter('hardlink')(target, {
    filesMap,
    force: false,
    resolvedFrom: 'remote',
  })).toBe('hardlink')

  const execDest = path.join(target, 'bin/cli.js')
  expect(mode(path.join(target, 'index.js'))).toBe(plainMode)
  expect(mode(path.join(target, 'package.json'))).toBe(plainMode)
  expect(mode(execDest)).toBe(execMode)
  expect(fs.readFileSync(execDest)).toEqual(EXEC_CONTENT)
  expect(inode(execDest)).not.toBe(inode(execSrc))
})

testOnLinuxOnly('copy tier imports store files at the current umask mode', () => {
  const installed = packageFromStore('copy', { exec: 'a', plain: 'b' })
  expect(mode(installed.plain)).toBe(desiredMode(false))
  expect(mode(installed.packageJson)).toBe(desiredMode(false))
  expect(mode(installed.execBin)).toBe(desiredMode(true))
  expect(fs.readFileSync(installed.execBin)).toEqual(EXEC_CONTENT)
})

testOnLinuxOnly('local directory files keep their own modes when copied', () => {
  // A local package can sit under a `files/` directory, so that component
  // alone cannot be the store signal.
  const srcDir = path.join(tempDir(), 'files/shared')
  fs.mkdirSync(srcDir, { recursive: true })
  const localExec = path.join(srcDir, 'tool-exec')
  fs.writeFileSync(localExec, EXEC_CONTENT)
  fs.chmodSync(localExec, 0o711)
  const localPlain = path.join(srcDir, 'index.js')
  fs.writeFileSync(localPlain, PLAIN_CONTENT)
  fs.chmodSync(localPlain, 0o600)
  const target = path.join(tempDir(), 'project/package')

  expect(createIndexedPkgImporter('copy')(target, {
    filesMap: new Map([
      ['bin/tool-exec', localExec],
      ['index.js', localPlain],
      ['package.json', localPlain],
    ]),
    force: false,
    resolvedFrom: 'remote',
  })).toBe('copy')

  expect(mode(path.join(target, 'bin/tool-exec'))).toBe(0o711)
  expect(mode(path.join(target, 'index.js'))).toBe(0o600)
})

testOnLinuxOnly('auto mode imports entries of either mode at the current umask', () => {
  const storeDir = path.join(tempDir(), 'store')
  const execMode = desiredMode(true)
  const plainMode = desiredMode(false)
  const filesMap = new Map<string, string>()
  addStoreEntry(storeDir, filesMap, 'bin/cli.js', 'a', true, execMode ^ 0b1, EXEC_CONTENT)
  const plainSrc = addStoreEntry(storeDir, filesMap, 'index.js', 'b', false, plainMode, PLAIN_CONTENT)
  addStoreEntry(storeDir, filesMap, 'package.json', 'c', false, plainMode, PKG_JSON_CONTENT)
  const target = path.join(tempDir(), 'project/package')

  const result = createIndexedPkgImporter('auto')(target, {
    filesMap,
    force: false,
    resolvedFrom: 'store',
  })
  expect(['clone', 'hardlink']).toContain(result)

  const execDest = path.join(target, 'bin/cli.js')
  const plainDest = path.join(target, 'index.js')
  expect(mode(execDest)).toBe(execMode)
  expect(mode(plainDest)).toBe(plainMode)
  if (result === 'hardlink') {
    expect(inode(plainDest)).toBe(inode(plainSrc))
  }
})

function addStoreEntry (
  storeDir: string,
  filesMap: Map<string, string>,
  name: string,
  seed: string,
  exec: boolean,
  mode: number,
  content: Buffer
): string {
  const dir = path.join(storeDir, 'files', '00')
  fs.mkdirSync(dir, { recursive: true })
  const file = path.join(dir, `${digest(seed)}${exec ? '-exec' : ''}`)
  fs.writeFileSync(file, content)
  fs.chmodSync(file, mode)
  filesMap.set(name, file)
  return file
}