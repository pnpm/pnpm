/// <reference path="../../../__typings__/index.d.ts"/>
import fs from 'node:fs'
import path from 'node:path'

import { expect, test } from '@jest/globals'
import { writeCurrentLockfile } from '@pnpm/lockfile.fs'
import { temporaryDirectory } from 'tempy'

import { getPatchedDependency } from '../src/getPatchedDependency.js'

// Regression test: when the only installed version of the patched package is
// git-hosted, getPatchedDependency must return the parsed dependency (which
// carries `alias`), not the options object. It previously spread `opts`, which
// dropped the package name and leaked unrelated option fields into the result.
test('getPatchedDependency preserves the alias for a single git-hosted version', async () => {
  const lockfileDir = fs.realpathSync(temporaryDirectory())
  const gitTarballUrl =
    'https://codeload.github.com/example/is-positive/tar.gz/0000000000000000000000000000000000000000'
  // Assigned to a variable first so the object literal is not subject to excess
  // property checks against LockfileObject's branded key types (matches the
  // pattern in lockfile/fs/test/read.test.ts).
  const lockfile = {
    importers: {
      '.': {
        dependencies: { 'is-positive': gitTarballUrl },
        specifiers: { 'is-positive': gitTarballUrl },
      },
    },
    lockfileVersion: '9.0',
    packages: {
      'is-positive@1.0.0': {
        version: '1.0.0',
        resolution: { tarball: gitTarballUrl },
      },
    },
    registry: 'https://registry.npmjs.org',
  }
  await writeCurrentLockfile(path.join(lockfileDir, 'node_modules', '.pnpm'), lockfile)

  const result = await getPatchedDependency('is-positive', {
    lockfileDir,
    modulesDir: 'node_modules',
    virtualStoreDir: path.join(lockfileDir, 'node_modules', '.pnpm'),
  })

  expect(result.alias).toBe('is-positive')
  expect(result.bareSpecifier).toBe(gitTarballUrl)
  expect(result.applyToAll).toBe(false)
})
