/// <reference path="../../../__typings__/index.d.ts"/>
import fs from 'node:fs'
import path from 'node:path'

import { expect, test } from '@jest/globals'
import { type Modules, readModulesManifest, writeModulesManifest } from '@pnpm/installing.modules-yaml'
import type { DepPath } from '@pnpm/types'
import isWindows from 'is-windows'
import { readYamlFileSync } from 'read-yaml-file'
import { temporaryDirectory } from 'tempy'

test('writeModulesManifest() and readModulesManifest()', async () => {
  const modulesDir = temporaryDirectory()
  const modulesYaml: Modules = {
    hoistedDependencies: {},
    included: {
      dependencies: true,
      devDependencies: true,
      optionalDependencies: true,
    },
    ignoredBuilds: new Set(),
    layoutVersion: 1,
    packageManager: 'pnpm@2',
    pendingBuilds: [],
    publicHoistPattern: [],
    prunedAt: new Date().toUTCString(),
    shamefullyHoist: false,
    skipped: [],
    storeDir: '/.pnpm-store',
    virtualStoreDir: path.join(modulesDir, '.pnpm'),
    virtualStoreDirMaxLength: 120,
  }
  await writeModulesManifest(modulesDir, modulesYaml)
  expect(await readModulesManifest(modulesDir)).toEqual(modulesYaml)

  const raw = readYamlFileSync<any>(path.join(modulesDir, '.modules.yaml')) // eslint-disable-line @typescript-eslint/no-explicit-any
  expect(raw.virtualStoreDir).toBeDefined()
  expect(path.isAbsolute(raw.virtualStoreDir)).toEqual(isWindows())
})

test('writeModulesManifest() and readModulesManifest() with a long dependency path', async () => {
  const modulesDir = temporaryDirectory()
  const longDepPath = `@scope/package@1.0.0(${'p'.repeat(1001)})` as DepPath
  const modulesYaml: Modules = {
    hoistedDependencies: {
      [longDepPath]: { '@scope/package': 'private' },
    },
    included: {
      dependencies: true,
      devDependencies: true,
      optionalDependencies: true,
    },
    ignoredBuilds: new Set(),
    layoutVersion: 1,
    packageManager: 'pnpm@2',
    pendingBuilds: [],
    publicHoistPattern: [],
    prunedAt: new Date().toUTCString(),
    shamefullyHoist: false,
    skipped: [],
    storeDir: '/.pnpm-store',
    virtualStoreDir: path.join(modulesDir, '.pnpm'),
    virtualStoreDirMaxLength: 120,
  }
  await writeModulesManifest(modulesDir, modulesYaml)
  expect(await readModulesManifest(modulesDir)).toEqual(modulesYaml)
})

test('readModulesManifest() resolves duplicate JSON keys to the last value', async () => {
  const modulesDir = temporaryDirectory()
  const longDepPath = `package@1.0.0(${'p'.repeat(1010)})`
  fs.writeFileSync(
    path.join(modulesDir, '.modules.yaml'),
    `{"hoistedDependencies":{"${longDepPath}":{"package":"private"},"${longDepPath}":{"package":"public"}}}`
  )
  const modulesYaml = await readModulesManifest(modulesDir)
  expect(modulesYaml?.hoistedDependencies).toEqual({ [longDepPath]: { package: 'public' } })
})

test('backward compatible read of .modules.yaml created with shamefully-hoist=true', async () => {
  const modulesYaml = await readModulesManifest(path.join(import.meta.dirname, 'fixtures/old-shamefully-hoist'))
  if (modulesYaml == null) {
    throw new Error('modulesYaml was nullish')
  }
  expect(modulesYaml.publicHoistPattern).toEqual(['*'])
  expect(modulesYaml.hoistedDependencies).toEqual({
    '/accepts/1.3.7': { accepts: 'public' },
    '/array-flatten/1.1.1': { 'array-flatten': 'public' },
    '/body-parser/1.19.0': { 'body-parser': 'public' },
  })
})

test('backward compatible read of .modules.yaml created with shamefully-hoist=false', async () => {
  const modulesYaml = await readModulesManifest(path.join(import.meta.dirname, 'fixtures/old-no-shamefully-hoist'))
  if (modulesYaml == null) {
    throw new Error('modulesYaml was nullish')
  }
  expect(modulesYaml.publicHoistPattern).toEqual([])
  expect(modulesYaml.hoistedDependencies).toEqual({
    '/accepts/1.3.7': { accepts: 'private' },
    '/array-flatten/1.1.1': { 'array-flatten': 'private' },
    '/body-parser/1.19.0': { 'body-parser': 'private' },
  })
})

test('readModulesManifest() should create a node_modules directory', async () => {
  const modulesDir = path.join(temporaryDirectory(), 'node_modules')
  const modulesYaml: Modules = {
    hoistedDependencies: {},
    included: {
      dependencies: true,
      devDependencies: true,
      optionalDependencies: true,
    },
    ignoredBuilds: new Set(),
    layoutVersion: 1,
    packageManager: 'pnpm@2',
    pendingBuilds: [],
    publicHoistPattern: [],
    prunedAt: new Date().toUTCString(),
    shamefullyHoist: false,
    skipped: [],
    storeDir: '/.pnpm-store',
    virtualStoreDir: path.join(modulesDir, '.pnpm'),
    virtualStoreDirMaxLength: 120,
  }
  await writeModulesManifest(modulesDir, modulesYaml)
  expect(await readModulesManifest(modulesDir)).toEqual(modulesYaml)
})

test('readModulesManifest does not fail on empty file', async () => {
  const modulesYaml = await readModulesManifest(path.join(import.meta.dirname, 'fixtures/empty-modules-yaml'))
  expect(modulesYaml).toBeUndefined()
})

test('readModulesManifest() rejects a manifest it cannot parse', async () => {
  // Callers must not mistake an unreadable state file for a missing one:
  // that reads as layout drift and purges node_modules on every install.
  const modulesDir = temporaryDirectory()
  fs.writeFileSync(path.join(modulesDir, '.modules.yaml'), 'not: [valid')
  await expect(readModulesManifest(modulesDir)).rejects.toThrow()
})

test('writeModulesManifest() drops the registries a pnpm 11 file recorded', async () => {
  // The registries a project resolves from are read from its config, so a
  // recorded copy is stale the moment the config changes.
  const modulesDir = temporaryDirectory()
  const modulesYaml: Modules = {
    hoistedDependencies: {},
    included: {
      dependencies: true,
      devDependencies: true,
      optionalDependencies: true,
    },
    layoutVersion: 1,
    packageManager: 'pnpm@2',
    pendingBuilds: [],
    prunedAt: new Date().toUTCString(),
    skipped: [],
    storeDir: '/.pnpm-store',
    virtualStoreDir: path.join(modulesDir, '.pnpm'),
    virtualStoreDirMaxLength: 120,
  }
  await writeModulesManifest(modulesDir, {
    ...modulesYaml,
    registries: { default: 'https://registry.npmjs.org/' },
  } as Modules)

  const raw = readYamlFileSync<Record<string, unknown>>(path.join(modulesDir, '.modules.yaml'))
  expect(raw.registries).toBeUndefined()
})
