import fs from 'node:fs'
import os from 'node:os'
import path from 'node:path'

import { expect, jest, test } from '@jest/globals'

import type { DependenciesGraph } from '../lib/buildSequence.js'

const hardLinkDir = jest.fn<(src: string, destDirs: string[]) => Promise<void>>(async () => {})
const originalWorker = await import('@pnpm/worker')
jest.unstable_mockModule('@pnpm/worker', () => ({
  ...originalWorker,
  hardLinkDir,
}))

const { buildModules } = await import('../lib/index.js')

// hardLinkDir() runs on a worker thread, and the cwd is process-wide: applyPatchToDir()
// switches the cwd to the package it patches, so a worker resolving a relative
// destination during that window resolves it against the wrong directory. That surfaced as
// intermittent ERR_PNPM_ENOENT/ENOTEMPTY renames of `_tmp_*` under nodeLinker=hoisted.
test('the hoisted locations passed to hardLinkDir are absolute', async () => {
  const lockfileDir = fs.mkdtempSync(path.join(os.tmpdir(), 'pnpm-hoisted-'))
  try {
    await runInstall(lockfileDir)
  } finally {
    fs.rmSync(lockfileDir, { recursive: true, force: true })
  }
})

async function runInstall (lockfileDir: string): Promise<void> {
  const pkgDir = path.join(lockfileDir, 'node_modules/foo')
  fs.mkdirSync(pkgDir, { recursive: true })
  fs.writeFileSync(path.join(pkgDir, 'package.json'), JSON.stringify({ name: 'foo', version: '1.0.0' }))

  const otherHoistedLocation = 'node_modules/bar/node_modules/foo'
  const depGraph = {
    'foo@1.0.0': {
      children: {},
      depPath: 'foo@1.0.0',
      name: 'foo',
      version: '1.0.0',
      dir: pkgDir,
      filesIndexFile: path.join(lockfileDir, 'index.json'),
      hasBin: false,
      hasBundledDependencies: false,
      optional: false,
      optionalDependencies: new Set<string>(),
      requiresBuild: true,
      isBuilt: false,
    },
  } as unknown as DependenciesGraph<string>

  await buildModules(depGraph, ['foo@1.0.0'], {
    depsStateCache: {},
    hoistedLocations: {
      'foo@1.0.0': ['node_modules/foo', otherHoistedLocation],
    },
    ignoreScripts: true,
    lockfileDir,
    optional: true,
    rootModulesDir: path.join(lockfileDir, 'node_modules'),
    sideEffectsCacheWrite: false,
    storeController: {} as never,
    unsafePerm: false,
    userAgent: 'pnpm',
  })

  expect(hardLinkDir).toHaveBeenCalledTimes(1)
  const destDirs = hardLinkDir.mock.calls[0][1]
  expect(destDirs).toStrictEqual([path.join(lockfileDir, otherHoistedLocation)])
  for (const destDir of destDirs) {
    expect(path.isAbsolute(destDir)).toBe(true)
  }
}
