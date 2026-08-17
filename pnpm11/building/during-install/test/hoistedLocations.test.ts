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

// A relative destination would race with applyPatchToDir()'s process-wide cwd switch —
// see the comment in buildDependency().
test('the hoisted locations passed to hardLinkDir are absolute', async () => {
  const lockfileDir = await fs.promises.mkdtemp(path.join(os.tmpdir(), 'pnpm-hoisted-'))
  try {
    await runInstall(lockfileDir)
  } finally {
    await fs.promises.rm(lockfileDir, { recursive: true, force: true })
  }
})

async function runInstall (lockfileDir: string): Promise<void> {
  // Real hoisted locations come from path.relative(), so they use the platform's
  // separator. Build the fixture's the same way, otherwise on Windows the install code
  // cannot match the current location against them and filters nothing out.
  const currentHoistedLocation = path.join('node_modules', 'foo')
  const otherHoistedLocation = path.join('node_modules', 'bar', 'node_modules', 'foo')
  const pkgDir = path.join(lockfileDir, currentHoistedLocation)
  await fs.promises.mkdir(pkgDir, { recursive: true })
  await fs.promises.writeFile(path.join(pkgDir, 'package.json'), JSON.stringify({ name: 'foo', version: '1.0.0' }))

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
      'foo@1.0.0': [currentHoistedLocation, otherHoistedLocation],
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
