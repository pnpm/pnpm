import fs from 'node:fs'
import { createRequire } from 'node:module'
import os from 'node:os'
import path from 'node:path'

import { expect, test } from '@jest/globals'
import { hoist } from '@pnpm/installing.linking.hoist'
import type { DepPath, ProjectId } from '@pnpm/types'
import { resolveLinkTarget } from 'resolve-link-target'
import { symlinkDir } from 'symlink-dir'

test('does not link skipped root dependencies into an external virtual store', async () => {
  const root = fs.realpathSync(fs.mkdtempSync(path.join(os.tmpdir(), 'pnpm-external-hoist-')))
  const virtualStoreDir = path.join(root, 'store')
  const depPath = '@types/fixture@1.0.0' as DepPath
  try {
    await hoist({
      graph: {
        [depPath]: {
          dir: path.join(virtualStoreDir, '@types+fixture@1.0.0/node_modules/@types/fixture'),
          children: {},
          optionalDependencies: new Set(),
          hasBin: false,
          name: '@types/fixture',
          depPath,
        },
      },
      directDepsByImporterId: {
        ['.' as ProjectId]: new Map([['@types/fixture', depPath]]),
      },
      skipped: new Set([depPath]),
      privateHoistPattern: ['*'],
      publicHoistPattern: [],
      privateHoistedModulesDir: path.join(virtualStoreDir, 'node_modules'),
      publicHoistedModulesDir: path.join(root, 'project/node_modules'),
      virtualStoreDir,
      virtualStoreDirMaxLength: 120,
    })

    expect(() => fs.lstatSync(path.join(virtualStoreDir, 'node_modules/@types/fixture')))
      .toThrow(expect.objectContaining({ code: 'ENOENT' }))
  } finally {
    fs.rmSync(root, { recursive: true, force: true })
  }
})

test.each([
  { store: 'external', privatePattern: ['*'], publicPattern: [], resolves: true },
  { store: 'internal', privatePattern: ['*'], publicPattern: [], resolves: true },
  { store: 'external', privatePattern: ['*'], publicPattern: ['*'], resolves: true },
  { store: 'external', privatePattern: ['*', '!@types/fixture'], publicPattern: [], resolves: false },
  { store: 'external', privatePattern: [], publicPattern: ['*'], resolves: false },
])('root dependency resolution with $store store, private $privatePattern and public $publicPattern', async ({ store, privatePattern, publicPattern, resolves }) => {
  const root = fs.realpathSync(fs.mkdtempSync(path.join(os.tmpdir(), 'pnpm-external-hoist-')))
  const modulesDir = path.join(root, 'project/node_modules')
  const virtualStoreDir = store === 'internal' ? path.join(modulesDir, '.pnpm') : path.join(root, 'store')
  const parent = path.join(virtualStoreDir, 'parent@1.0.0/node_modules/parent')
  const types = path.join(virtualStoreDir, '@types+fixture@1.0.0/node_modules/@types/fixture')
  const otherTypes = path.join(virtualStoreDir, '@types+fixture@2.0.0/node_modules/@types/fixture')
  fs.mkdirSync(parent, { recursive: true })
  fs.mkdirSync(types, { recursive: true })
  fs.mkdirSync(otherTypes, { recursive: true })
  fs.writeFileSync(path.join(types, 'index.d.ts'), 'export interface Fixture {}')
  fs.writeFileSync(path.join(otherTypes, 'index.d.ts'), 'export interface OtherFixture {}')
  await symlinkDir(parent, path.join(modulesDir, 'parent'))
  await symlinkDir(types, path.join(modulesDir, '@types/fixture'))
  try {
    await hoist({
      graph: {
        parent: {
          dir: parent,
          children: {},
          optionalDependencies: new Set(),
          hasBin: false,
          name: 'parent',
          depPath: 'parent@1.0.0' as DepPath,
        },
        types: {
          dir: types,
          children: {},
          optionalDependencies: new Set(),
          hasBin: false,
          name: '@types/fixture',
          depPath: '@types/fixture@1.0.0' as DepPath,
        },
        'other-types': {
          dir: otherTypes,
          children: {},
          optionalDependencies: new Set(),
          hasBin: false,
          name: '@types/fixture',
          depPath: '@types/fixture@2.0.0' as DepPath,
        },
      },
      directDepsByImporterId: {
        ['workspace' as ProjectId]: new Map([['@types/fixture', 'other-types']]),
        ['.' as ProjectId]: new Map([['parent', 'parent'], ['@types/fixture', 'types']]),
      },
      skipped: new Set<DepPath>(),
      privateHoistPattern: privatePattern,
      publicHoistPattern: publicPattern,
      privateHoistedModulesDir: path.join(virtualStoreDir, 'node_modules'),
      publicHoistedModulesDir: modulesDir,
      virtualStoreDir,
      virtualStoreDirMaxLength: 120,
    })

    const resolveTypes = () => createRequire(path.join(parent, 'index.js')).resolve('@types/fixture/index.d.ts')
    if (resolves) {
      expect(resolveTypes()).toBe(path.join(types, 'index.d.ts'))
    } else {
      expect(resolveTypes).toThrow("Cannot find module '@types/fixture/index.d.ts'")
    }
    expect(await resolveLinkTarget(path.join(modulesDir, '@types/fixture'))).toBe(types)
  } finally {
    fs.rmSync(root, { recursive: true, force: true })
  }
})
