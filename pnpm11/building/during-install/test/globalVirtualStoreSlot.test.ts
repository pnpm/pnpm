import fs from 'node:fs'
import os from 'node:os'
import path from 'node:path'

import { afterEach, beforeEach, expect, test } from '@jest/globals'
import { getStagingDir } from '@pnpm/deps.graph-hasher'

import type { DependenciesGraphNode } from '../lib/buildGraph.js'
import {
  createStagingToken,
  placeBuiltPackage,
  replacePlacedPackage,
  selectImportDir,
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
  expect(getStagingDir('/slot/node_modules/foo', 'tok')).toBe(path.normalize('/slot/node_modules/.foo-tok'))
  expect(getStagingDir('/slot/node_modules/@scope/foo', 'tok')).toBe(path.normalize('/slot/node_modules/@scope/.foo-tok'))
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

  expect(await selectImportDir(pkg, { hasCachedBuild: false, opts })).toBe(getStagingDir(pkgDir, 'tok'))
  expect(await selectImportDir(pkg, { hasCachedBuild: true, opts })).toBe(pkgDir)
  expect(await selectImportDir(pkg, { hasCachedBuild: false, opts: { ...opts, ignoreScripts: true } })).toBe(pkgDir)
  expect(await selectImportDir(pkg, { hasCachedBuild: false, opts: { ...opts, allowBuild: () => undefined } })).toBe(pkgDir)
  expect(await selectImportDir({ ...pkg, requiresBuild: false, patch: {} }, { hasCachedBuild: false, opts: { ...opts, ignoreScripts: true } }))
    .toBe(getStagingDir(pkgDir, 'tok'))

  writePackage(pkgDir, { 'package.json': '{}' })
  expect(await selectImportDir(pkg, { hasCachedBuild: false, opts })).toBeUndefined()
  // A package from a local directory is staged again, for its build to replace the placed package.
  expect(await selectImportDir({ ...pkg, resolution: { type: 'directory', directory: '../foo' } }, { hasCachedBuild: true, opts }))
    .toBe(getStagingDir(pkgDir, 'tok'))
})

test('createStagingToken() carries random bits', () => {
  expect(createStagingToken()).toMatch(/^[\da-f]+-[\da-f]+-[\da-f]{16}$/)
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
