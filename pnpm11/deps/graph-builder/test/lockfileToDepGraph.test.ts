/// <reference path="../../../__typings__/index.d.ts" />
import fs from 'node:fs'
import os from 'node:os'
import path from 'node:path'

import { expect, test } from '@jest/globals'
import type { LockfileObject } from '@pnpm/lockfile.fs'

import { lockfileToDepGraph, type LockfileToDepGraphOptions } from '../src/lockfileToDepGraph.js'

// A crafted lockfile whose `packages` depPath *key* carries a path-traversal
// (or reserved) name. The isolated (virtual-store) linker reconstructs the
// package name from that key via `dp.parse` and joins it onto the virtual
// store to form the package's install directory, so this is the shape an
// attacker who can ship a lockfile would use to make `pnpm install` write
// package content outside the store (GHSA-c59q-g84q-2gj5).
function craftedLockfile (name: string): LockfileObject {
  return {
    lockfileVersion: '9.0',
    importers: {
      '.': {
        dependencies: { 'legit-name': `${name}@1.0.0` },
        specifiers: { 'legit-name': '1.0.0' },
      },
    },
    packages: {
      [`${name}@1.0.0`]: {
        resolution: { integrity: 'sha512-deadbeef' },
      },
    },
  } as unknown as LockfileObject
}

// `force: true` skips the installability check so the walk reaches the name
// sink directly; the store controller throws if touched, proving the name is
// rejected before any fetch or filesystem work.
function graphOpts (lockfileDir: string): LockfileToDepGraphOptions {
  const unreachable = (name: string) => () => {
    throw new Error(`${name} must not be reached for a rejected package name`)
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
    registries: { default: 'http://localhost/' },
    sideEffectsCacheRead: false,
    skipped: new Set(),
    storeController: {
      fetchPackage: unreachable('fetchPackage'),
      getFilesIndexFilePath: unreachable('getFilesIndexFilePath'),
    },
    storeDir: path.join(lockfileDir, 'store'),
    globalVirtualStoreDir: path.join(lockfileDir, 'store', 'v10'),
    virtualStoreDir: path.join(lockfileDir, 'node_modules', '.pnpm'),
    virtualStoreDirMaxLength: 120,
  } as unknown as LockfileToDepGraphOptions
}

test.each([
  '../../../escape',
  '@scope/../../escape',
  '.bin',
  '.pnpm',
  'node_modules',
])('lockfileToDepGraph rejects package name %p', async (name) => {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'graph-builder-'))
  await expect(
    lockfileToDepGraph(craftedLockfile(name), null, graphOpts(dir))
  ).rejects.toThrow(expect.objectContaining({ code: 'ERR_PNPM_INVALID_DEPENDENCY_NAME' }))
})

test('lockfileToDepGraph does not create a directory outside the virtual store for a traversal name', async () => {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'graph-builder-'))
  const escaped = path.join(dir, 'node_modules', '.pnpm', '..', '..', '..', 'escape')
  await expect(
    lockfileToDepGraph(craftedLockfile('../../../escape'), null, graphOpts(dir))
  ).rejects.toThrow(expect.objectContaining({ code: 'ERR_PNPM_INVALID_DEPENDENCY_NAME' }))
  expect(fs.existsSync(escaped)).toBe(false)
})
