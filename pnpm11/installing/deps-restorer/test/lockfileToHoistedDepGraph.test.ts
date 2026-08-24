/// <reference path="../../../__typings__/index.d.ts" />
import fs from 'node:fs'
import path from 'node:path'

import { expect, test } from '@jest/globals'
import type { LockfileObject } from '@pnpm/lockfile.fs'
import { tempDir } from '@pnpm/prepare'

import { lockfileToHoistedDepGraph } from '../src/lockfileToHoistedDepGraph.js'

// A crafted lockfile whose dependency *alias* (the key pnpm turns into a
// `node_modules/<alias>` directory) is a path-traversal or reserved name,
// pointing at an otherwise ordinary package snapshot. The `nodeLinker:
// hoisted` restore path reads aliases straight from the lockfile, so this
// is the shape an attacker who can ship a lockfile would use to escape
// `node_modules` or overwrite pnpm-owned layout (`.bin` / `.pnpm`).
function craftedLockfile (alias: string): LockfileObject {
  return {
    lockfileVersion: '9.0',
    importers: {
      '.': {
        dependencies: { [alias]: '1.0.0' },
        specifiers: { [alias]: '1.0.0' },
      },
    },
    packages: {
      [`${alias}@1.0.0`]: {
        resolution: { integrity: 'sha512-deadbeef' },
      },
    },
  } as unknown as LockfileObject
}

// `force: true` skips the installability check so the walk reaches the
// alias sink directly; the store controller throws if touched, proving
// the alias is rejected before any fetch or filesystem work.
function hoistedOpts (lockfileDir: string): Parameters<typeof lockfileToHoistedDepGraph>[2] {
  const unreachable = (name: string) => () => {
    throw new Error(`${name} must not be reached for a rejected alias`)
  }
  return {
    autoInstallPeers: false,
    engineStrict: false,
    force: true,
    importerIds: ['.'],
    include: { dependencies: true, devDependencies: true, optionalDependencies: true },
    ignoreScripts: false,
    lockfileDir,
    nodeVersion: process.version,
    pnpmVersion: '0.0.0',
    registriesByScope: { default: 'http://localhost/' },
    requiredDepPaths: new Set(),
    sideEffectsCacheRead: false,
    skipped: new Set<string>(),
    storeController: {
      fetchPackage: unreachable('fetchPackage'),
      getFilesIndexFilePath: unreachable('getFilesIndexFilePath'),
    },
    storeDir: path.join(lockfileDir, 'store'),
    virtualStoreDir: path.join(lockfileDir, 'node_modules', '.pnpm'),
  } as unknown as Parameters<typeof lockfileToHoistedDepGraph>[2]
}

test.each([
  '../../../escape',
  '@scope/../../escape',
  '.bin',
  '.pnpm',
  'node_modules',
])('lockfileToHoistedDepGraph rejects hoisted alias %p', async (alias) => {
  const dir = tempDir(false)
  await expect(
    lockfileToHoistedDepGraph(craftedLockfile(alias), null, hoistedOpts(dir))
  ).rejects.toThrow(expect.objectContaining({ code: 'ERR_PNPM_INVALID_DEPENDENCY_NAME' }))
})

test('lockfileToHoistedDepGraph does not create a file outside node_modules for a traversal alias', async () => {
  const dir = tempDir(false)
  const escaped = path.join(dir, 'node_modules', '..', '..', '..', 'escape')
  await expect(
    lockfileToHoistedDepGraph(craftedLockfile('../../../escape'), null, hoistedOpts(dir))
  ).rejects.toThrow(expect.objectContaining({ code: 'ERR_PNPM_INVALID_DEPENDENCY_NAME' }))
  expect(fs.existsSync(escaped)).toBe(false)
})

// Two peer variants of one version collapse onto a single hoister node
// keyed by the first depPath seen for the version, so the walk never
// records a location under the other variant's depPath. An edge declared
// against that variant — here `c`'s dependency on `b` — still has to
// resolve to the copy the surviving node produced, or `b` drops out of
// `c`'s children and off `c`'s `node_modules/.bin`.
test('lockfileToHoistedDepGraph wires an edge declared against a collapsed peer variant', async () => {
  const dir = tempDir(false)
  const opts = hoistedOpts(dir)
  opts.storeController = {
    fetchPackage: () => ({ filesIndexFile: '' }),
    getFilesIndexFilePath: () => ({ filesIndexFile: '' }),
  } as unknown as typeof opts.storeController

  const { graph } = await lockfileToHoistedDepGraph(peerVariantLockfile(), null, opts)

  const modulesDir = path.join(dir, 'node_modules')
  expect(graph[path.join(modulesDir, 'c')].children.b).toBe(path.join(modulesDir, 'b'))
})

function peerVariantLockfile (): LockfileObject {
  return {
    lockfileVersion: '9.0',
    importers: {
      '.': {
        dependencies: {
          b: '1.0.0(peer@2.0.0)',
          c: '1.0.0',
          peer: '2.0.0',
        },
        specifiers: { b: '1.0.0', c: '1.0.0', peer: '2.0.0' },
      },
    },
    packages: {
      'b@1.0.0(peer@2.0.0)': {
        resolution: { integrity: 'sha512-deadbeef' },
        dependencies: { peer: '2.0.0' },
      },
      'b@1.0.0(peer@3.0.0)': {
        resolution: { integrity: 'sha512-deadbeef' },
        dependencies: { peer: '3.0.0' },
      },
      'c@1.0.0': {
        resolution: { integrity: 'sha512-deadbeef' },
        dependencies: { b: '1.0.0(peer@3.0.0)' },
      },
      'peer@2.0.0': { resolution: { integrity: 'sha512-deadbeef' } },
      'peer@3.0.0': { resolution: { integrity: 'sha512-deadbeef' } },
    },
  } as unknown as LockfileObject
}

// Peer variants of an injected directory dependency are exempt from the
// collapse (see `getHoisterPkgId`), so the walk has to keep a location
// per variant, where every collapsed package funnels into one.
test('lockfileToHoistedDepGraph keeps file-dep peer variants apart', async () => {
  const dir = tempDir(false)
  const opts = hoistedOpts(dir)
  opts.storeController = {
    fetchPackage: () => ({ filesIndexFile: '' }),
    getFilesIndexFilePath: () => ({ filesIndexFile: '' }),
  } as unknown as typeof opts.storeController

  const { graph } = await lockfileToHoistedDepGraph(fileVariantLockfile(), null, opts)

  const compDirs = Object.keys(graph).filter((dir) => path.basename(dir) === 'comp')
  expect(compDirs).toHaveLength(2)
  const peerVersionByVariant = Object.fromEntries(compDirs.map((dir) => {
    const variant = graph[dir].depPath
    const peerDir = graph[dir].children.peer
    return [variant, graph[peerDir].depPath]
  }))
  expect(peerVersionByVariant).toStrictEqual({
    'comp@file:comp(peer@1.0.0)': 'peer@1.0.0',
    'comp@file:comp(peer@2.0.0)': 'peer@2.0.0',
  })
})

function fileVariantLockfile (): LockfileObject {
  const importer = (peerVersion: string) => ({
    dependencies: {
      comp: `file:comp(peer@${peerVersion})`,
      peer: peerVersion,
    },
    specifiers: { comp: 'workspace:*', peer: peerVersion },
  })
  const compVariant = (peerVersion: string) => ({
    resolution: { directory: 'comp', type: 'directory' },
    dependencies: { peer: peerVersion },
  })
  return {
    lockfileVersion: '9.0',
    importers: {
      '.': { specifiers: {} },
      'node_modules/.bit_roots/r1': importer('1.0.0'),
      'node_modules/.bit_roots/r2': importer('2.0.0'),
    },
    packages: {
      'comp@file:comp(peer@1.0.0)': compVariant('1.0.0'),
      'comp@file:comp(peer@2.0.0)': compVariant('2.0.0'),
      'peer@1.0.0': { resolution: { integrity: 'sha512-deadbeef' } },
      'peer@2.0.0': { resolution: { integrity: 'sha512-deadbeef' } },
    },
  } as unknown as LockfileObject
}
