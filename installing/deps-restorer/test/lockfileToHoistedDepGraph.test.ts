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
    registries: { default: 'http://localhost/' },
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
