import fs from 'node:fs'
import os from 'node:os'
import path from 'node:path'

import { afterEach, beforeEach, expect, test } from '@jest/globals'
import { createStagingToken, getStagingDir } from '@pnpm/deps.graph-hasher'

import type { DependenciesGraphNode } from '../lib/buildGraph.js'
import {
  type CacheRow,
  getCacheRow,
  placeBuiltPackage,
  replacePlacedPackage,
  selectImportDir,
  type SlotImportOptions,
} from '../lib/globalVirtualStoreSlot.js'
import { removeLinksToSkippedOptionalDependency } from '../lib/removeSkippedDependency.js'

let slotModulesDir: string

beforeEach(async () => {
  slotModulesDir = path.join(await fs.promises.mkdtemp(path.join(os.tmpdir(), 'pnpm-slot-')), 'node_modules')
  await fs.promises.mkdir(slotModulesDir)
})

afterEach(async () => {
  await fs.promises.rm(path.dirname(slotModulesDir), { recursive: true, force: true })
})

function writePackage (dir: string, files: Record<string, string>): void {
  fs.mkdirSync(dir, { recursive: true })
  for (const [name, content] of Object.entries(files)) {
    fs.writeFileSync(path.join(dir, name), content)
  }
}

test('getStagingDir() names a staging directory beside the package, inside its scope', () => {
  expect(getStagingDir('/slot/node_modules/foo', 'tok')).toBe(path.normalize('/slot/node_modules/.pnpm-tok'))
  expect(getStagingDir('/slot/node_modules/@scope/foo', 'tok')).toBe(path.normalize('/slot/node_modules/@scope/.pnpm-tok'))
  expect(createStagingToken()).not.toBe(createStagingToken())
})

test('placeBuiltPackage() renames the staging directory onto a missing package directory', async () => {
  const pkgDir = path.join(slotModulesDir, '@scope/foo')
  const stagingDir = getStagingDir(pkgDir, createStagingToken())
  writePackage(stagingDir, { 'package.json': '{}', 'built.txt': 'built' })

  expect(await placeBuiltPackage(stagingDir, pkgDir)).toBe('renamed')

  expect(fs.readFileSync(path.join(pkgDir, 'built.txt'), 'utf8')).toBe('built')
  expect(fs.existsSync(stagingDir)).toBe(false)
})

test('placeBuiltPackage() keeps a package another install placed first', async () => {
  const pkgDir = path.join(slotModulesDir, 'foo')
  writePackage(pkgDir, { 'package.json': '{}', 'built.txt': 'first' })
  const stagingDir = getStagingDir(pkgDir, createStagingToken())
  writePackage(stagingDir, { 'package.json': '{}', 'built.txt': 'second' })

  expect(await placeBuiltPackage(stagingDir, pkgDir)).toBe('placedByOther')

  expect(fs.readFileSync(path.join(pkgDir, 'built.txt'), 'utf8')).toBe('first')
  // The caller discards the staging directory.
  expect(fs.existsSync(stagingDir)).toBe(true)
})

test('placeBuiltPackage() moves aside a package directory without package.json', async () => {
  const pkgDir = path.join(slotModulesDir, 'foo')
  writePackage(pkgDir, { 'partial.txt': 'unfinished import' })
  const stagingDir = getStagingDir(pkgDir, createStagingToken())
  writePackage(stagingDir, { 'package.json': '{}', 'built.txt': 'built' })

  await placeBuiltPackage(stagingDir, pkgDir)

  expect(fs.readdirSync(pkgDir).sort()).toStrictEqual(['built.txt', 'package.json'])
  expect(fs.readdirSync(slotModulesDir)).toStrictEqual(['foo'])
})

test('replacePlacedPackage() swaps a placed package for a rebuilt one', async () => {
  const pkgDir = path.join(slotModulesDir, 'foo')
  writePackage(pkgDir, { 'package.json': '{}', 'built.txt': 'old' })
  const stagingDir = getStagingDir(pkgDir, createStagingToken())
  writePackage(stagingDir, { 'package.json': '{}', 'built.txt': 'new' })

  await replacePlacedPackage(stagingDir, pkgDir)

  expect(fs.readFileSync(path.join(pkgDir, 'built.txt'), 'utf8')).toBe('new')
  expect(fs.readdirSync(slotModulesDir)).toStrictEqual(['foo'])
})

test('selectImportDir() stages only a package that this install builds and that is not placed', async () => {
  const pkgDir = path.join(slotModulesDir, 'foo')
  const pkg = { depPath: 'foo@1.0.0' as never, dir: pkgDir, requiresBuild: true }
  const opts = { allowBuild: () => true, enableGlobalVirtualStore: true, stagingToken: 'tok' }
  const select = async (selected: Parameters<typeof selectImportDir>[0], cacheRow: CacheRow, selectOpts: SlotImportOptions = opts) =>
    selectImportDir(selected, { cacheRow, opts: selectOpts })
  const staged = { dir: getStagingDir(pkgDir, 'tok'), buildsForOutsideEffects: false }
  const inPlace = { dir: pkgDir, buildsForOutsideEffects: false }

  expect(await select(pkg, 'none')).toStrictEqual(staged)
  expect(await select(pkg, 'empty')).toStrictEqual(staged)
  expect(await select(pkg, 'files')).toStrictEqual(inPlace)
  expect(await select(pkg, 'none', { ...opts, ignoreScripts: true })).toStrictEqual(inPlace)
  expect(await select(pkg, 'none', { ...opts, enableModulesDir: false })).toStrictEqual(inPlace)
  expect(await select(pkg, 'none', { ...opts, allowBuild: () => undefined })).toStrictEqual(inPlace)
  const patched = { ...pkg, requiresBuild: false, patch: {} }
  expect(await select(patched, 'none', { ...opts, ignoreScripts: true })).toStrictEqual(staged)
  // An install without a modules directory has no build phase to apply the patch.
  expect(await select(patched, 'none', { ...opts, enableModulesDir: false })).toBeUndefined()

  writePackage(pkgDir, { 'package.json': '{}' })
  expect(await select(pkg, 'none')).toBeUndefined()
  expect(await select(pkg, 'files')).toBeUndefined()
  // A placed package whose build changed nothing in it is built again for its effects outside it.
  expect(await select(pkg, 'empty')).toStrictEqual({ ...staged, buildsForOutsideEffects: true })
  // A package from a local directory is staged again, for its build to replace the placed package.
  expect(await select({ ...pkg, resolution: { type: 'directory', directory: '../foo' } }, 'files')).toStrictEqual(staged)
})

test('getCacheRow() tells an empty row from one with files', () => {
  const files = { emptySideEffects: ['empty-key'], sideEffectsMaps: new Map([['empty-key', {}], ['files-key', {}]]) }
  expect(getCacheRow(files, undefined)).toBe('none')
  expect(getCacheRow(files, 'missing-key')).toBe('none')
  expect(getCacheRow(files, 'empty-key')).toBe('empty')
  expect(getCacheRow(files, 'files-key')).toBe('files')
})

test('createStagingToken() carries random bits', () => {
  expect(createStagingToken()).toMatch(/^[\da-f]{8}$/)
})

test('removeLinksToSkippedOptionalDependency() removes links to a package that was never placed', async () => {
  const pkgDir = path.join(slotModulesDir, 'foo')
  const projectModulesDir = path.join(path.dirname(slotModulesDir), 'project/node_modules')
  fs.mkdirSync(projectModulesDir, { recursive: true })
  await fs.promises.symlink(pkgDir, path.join(projectModulesDir, 'foo'), 'junction')

  await removeLinksToSkippedOptionalDependency(
    { depPath: 'foo@1.0.0', dir: pkgDir } as unknown as DependenciesGraphNode<string>,
    { linkedModulesDirs: [projectModulesDir] }
  )

  expect(fs.lstatSync(path.join(projectModulesDir, 'foo'), { throwIfNoEntry: false })).toBeUndefined()
})
