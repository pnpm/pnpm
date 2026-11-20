import fs from 'node:fs'
import os from 'node:os'
import path from 'node:path'

import { afterAll, expect, test } from '@jest/globals'
import type { PackageFilesIndex } from '@pnpm/store.cafs'
import { StoreIndex } from '@pnpm/store.index'

import { addFilesFromDir, finishWorkers, readPkgFromCafs } from '../lib/index.js'

afterAll(() => finishWorkers())

test('a stored requiresBuild that predates gypfile is rechecked against the package.json in the store', async () => {
  expect(await readLegacyRowRequiresBuild({ name: 'opted-out', version: '1.0.0', gypfile: false })).toBe(false)
  expect(await readLegacyRowRequiresBuild({ name: 'not-opted-out', version: '1.0.0' })).toBe(true)
})

test('a package.json with a byte-order mark is rechecked like any other', async () => {
  expect(await readLegacyRowRequiresBuild({ name: 'bom', version: '1.0.0', gypfile: false }, '\uFEFF')).toBe(false)
})

test('a row without requiresBuild or a bundled manifest reads the install scripts from the package.json in the store', async () => {
  const tmp = fs.mkdtempSync(path.join(os.tmpdir(), 'pnpm-stored-requires-build-'))
  const dir = path.join(tmp, 'pkg')
  fs.mkdirSync(dir)
  fs.writeFileSync(path.join(dir, 'package.json'), JSON.stringify({ name: 'scripted', version: '1.0.0', scripts: { postinstall: 'node build.js' } }))
  const storeDir = path.join(tmp, 'store')
  const storeIndex = new StoreIndex(storeDir)
  const filesIndexFile = path.join(storeDir, 'pkg.json')
  await addFilesFromDir({ storeDir, dir, filesIndexFile, storeIndex })

  const { manifest: _manifest, requiresBuild: _requiresBuild, ...row } = storeIndex.get(filesIndexFile) as PackageFilesIndex
  storeIndex.set(filesIndexFile, row)

  const result = await readPkgFromCafs({ storeDir, verifyStoreIntegrity: false }, filesIndexFile)
  storeIndex.close()
  expect(result.files.requiresBuild).toBe(true)
})

test('a git row from an older pnpm v12 with install scripts is re-derived as build work', async () => {
  expect(await readLegacyGitRowRequiresBuild({
    manifest: { name: 'scripted', version: '1.0.0', scripts: { postinstall: 'node build.js' } },
    requiresPrepare: false,
    storedRequiresBuild: false,
  })).toBe(true)
  expect(await readLegacyGitRowRequiresBuild({
    manifest: { name: 'scripted', version: '1.0.0', scripts: { postinstall: 'node build.js' } },
    requiresPrepare: true,
    storedRequiresBuild: false,
  })).toBe(true)
})

test('a git row from an older pnpm v12 without build triggers is re-derived as no build', async () => {
  expect(await readLegacyGitRowRequiresBuild({
    manifest: { name: 'plain', version: '1.0.0' },
    requiresPrepare: false,
    storedRequiresBuild: true,
  })).toBe(false)
  expect(await readLegacyGitRowRequiresBuild({
    manifest: { name: 'plain', version: '1.0.0', scripts: { postinstall: '' } },
    requiresPrepare: true,
    storedRequiresBuild: true,
  })).toBe(false)
})

test('a git row from an older pnpm v12 with a binding.gyp is re-derived as build work', async () => {
  expect(await readLegacyGitRowRequiresBuild({
    files: { 'binding.gyp': '{}' },
    manifest: { name: 'gyp', version: '1.0.0' },
    requiresPrepare: false,
    storedRequiresBuild: false,
  })).toBe(true)
})

test('a git row from an older pnpm v12 with gypfile false and a binding.gyp is re-derived as no build', async () => {
  expect(await readLegacyGitRowRequiresBuild({
    files: { 'binding.gyp': '{}' },
    manifest: { name: 'gyp-opted-out', version: '1.0.0', gypfile: false },
    requiresPrepare: false,
    storedRequiresBuild: true,
  })).toBe(false)
})

test('a git row from an older pnpm v12 with a .hooks entry is re-derived as build work', async () => {
  expect(await readLegacyGitRowRequiresBuild({
    files: { '.hooks/build.js': 'module.exports = 1' },
    manifest: { name: 'hooked', version: '1.0.0' },
    requiresPrepare: false,
    storedRequiresBuild: false,
  })).toBe(true)
})

test('a git row that bundles a manifest keeps its stored requiresBuild', async () => {
  expect(await readLegacyGitRowRequiresBuild({
    manifest: { name: 'scripted', version: '1.0.0', scripts: { postinstall: 'node build.js' } },
    manifestInRow: true,
    requiresPrepare: false,
    storedRequiresBuild: false,
  })).toBe(false)
})

test('a row without requiresPrepare keeps its stored requiresBuild', async () => {
  expect(await readLegacyGitRowRequiresBuild({
    manifest: { name: 'scripted', version: '1.0.0', scripts: { postinstall: 'node build.js' } },
    storedRequiresBuild: false,
  })).toBe(false)
})

test('a git row from an older pnpm v12 is re-derived when store verification runs', async () => {
  expect(await readLegacyGitRowRequiresBuild({
    manifest: { name: 'scripted', version: '1.0.0', scripts: { postinstall: 'node build.js' } },
    requiresPrepare: false,
    storedRequiresBuild: false,
    verifyStoreIntegrity: true,
  })).toBe(true)
})

const testIfReadPermissionsApply = process.platform === 'win32' || process.getuid?.() === 0 ? test.skip : test

testIfReadPermissionsApply('an entry that fails verification is reported without reading its package.json', async () => {
  const tmp = fs.mkdtempSync(path.join(os.tmpdir(), 'pnpm-stored-requires-build-'))
  const dir = path.join(tmp, 'pkg')
  fs.mkdirSync(dir)
  fs.writeFileSync(path.join(dir, 'package.json'), JSON.stringify({ name: 'broken', version: '1.0.0' }))
  fs.writeFileSync(path.join(dir, 'index.js'), 'module.exports = 1')
  const storeDir = path.join(tmp, 'store')
  const storeIndex = new StoreIndex(storeDir)
  const filesIndexFile = path.join(storeDir, 'pkg.json')
  await addFilesFromDir({ storeDir, dir, filesIndexFile, storeIndex })
  const { manifest: _manifest, requiresBuild: _requiresBuild, ...row } = storeIndex.get(filesIndexFile) as PackageFilesIndex
  storeIndex.set(filesIndexFile, row)
  const { files } = await readPkgFromCafs({ storeDir, verifyStoreIntegrity: false }, filesIndexFile)
  fs.rmSync(files.filesMap.get('index.js')!)
  const manifestPath = files.filesMap.get('package.json')!
  fs.chmodSync(manifestPath, 0o000)

  try {
    const result = await readPkgFromCafs({ storeDir, verifyStoreIntegrity: true }, filesIndexFile)
    expect(result.verified).toBe(false)
  } finally {
    fs.chmodSync(manifestPath, 0o644)
    storeIndex.close()
  }
})

async function readLegacyRowRequiresBuild (manifest: Record<string, unknown>, prefix = ''): Promise<boolean> {
  const tmp = fs.mkdtempSync(path.join(os.tmpdir(), 'pnpm-stored-requires-build-'))
  const dir = path.join(tmp, 'pkg')
  fs.mkdirSync(dir)
  fs.writeFileSync(path.join(dir, 'package.json'), prefix + JSON.stringify(manifest))
  fs.writeFileSync(path.join(dir, 'binding.gyp'), '{}')
  const storeDir = path.join(tmp, 'store')
  const storeIndex = new StoreIndex(storeDir)
  const filesIndexFile = path.join(storeDir, 'pkg.json')
  await addFilesFromDir({ storeDir, dir, filesIndexFile, storeIndex })

  const row = storeIndex.get(filesIndexFile) as PackageFilesIndex
  delete row.manifest?.gypfile
  storeIndex.set(filesIndexFile, { ...row, requiresBuild: true })

  const result = await readPkgFromCafs({ storeDir, verifyStoreIntegrity: false }, filesIndexFile)
  storeIndex.close()
  return result.files.requiresBuild
}

interface LegacyGitRowOptions {
  files?: Record<string, string>
  manifest: Record<string, unknown>
  manifestInRow?: boolean
  requiresPrepare?: boolean
  storedRequiresBuild: boolean
  verifyStoreIntegrity?: boolean
}

async function readLegacyGitRowRequiresBuild ({
  files = {},
  manifest,
  manifestInRow = false,
  requiresPrepare,
  storedRequiresBuild,
  verifyStoreIntegrity = false,
}: LegacyGitRowOptions): Promise<boolean> {
  const tmp = fs.mkdtempSync(path.join(os.tmpdir(), 'pnpm-stored-requires-build-'))
  const dir = path.join(tmp, 'pkg')
  fs.mkdirSync(dir)
  fs.writeFileSync(path.join(dir, 'package.json'), JSON.stringify(manifest))
  for (const [filename, contents] of Object.entries(files)) {
    const filePath = path.join(dir, filename)
    fs.mkdirSync(path.dirname(filePath), { recursive: true })
    fs.writeFileSync(filePath, contents)
  }
  const storeDir = path.join(tmp, 'store')
  const storeIndex = new StoreIndex(storeDir)
  const filesIndexFile = path.join(storeDir, 'pkg.json')
  await addFilesFromDir({ storeDir, dir, filesIndexFile, storeIndex, requiresPrepare })

  const row = storeIndex.get(filesIndexFile) as PackageFilesIndex
  if (!manifestInRow) delete row.manifest
  storeIndex.set(filesIndexFile, { ...row, requiresBuild: storedRequiresBuild })

  const result = await readPkgFromCafs({ storeDir, verifyStoreIntegrity }, filesIndexFile)
  storeIndex.close()
  return result.files.requiresBuild
}
