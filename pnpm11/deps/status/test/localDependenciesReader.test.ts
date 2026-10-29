import fs from 'node:fs/promises'
import os from 'node:os'
import path from 'node:path'

import { afterEach, expect, test } from '@jest/globals'
import { createLocalDependenciesReader } from '@pnpm/deps.status'
import { type LockfileObject, writeCurrentLockfile } from '@pnpm/lockfile.fs'
import type { DepPath, ProjectId } from '@pnpm/types'

const dirs: string[] = []

afterEach(async () => {
  await Promise.all(dirs.splice(0).map(async (dir) => fs.rm(dir, { recursive: true, force: true })))
})

async function tempDir (): Promise<string> {
  const dir = await fs.realpath(await fs.mkdtemp(path.join(os.tmpdir(), 'pnpm-local-dependencies-reader-')))
  dirs.push(dir)
  return dir
}

const lockfile: LockfileObject = {
  lockfileVersion: '9.0',
  importers: {
    ['.' as ProjectId]: {
      specifiers: { foo: '1.0.0', lib: 'file:../lib', pkg: 'file:vendor/pkg.tgz' },
      dependencies: { foo: '1.0.0', lib: 'file:../lib', pkg: 'file:vendor/pkg.tgz' },
    },
  },
  packages: {
    ['foo@1.0.0' as DepPath]: { resolution: { integrity: 'sha512-foo' } },
    ['lib@file:../lib' as DepPath]: { resolution: { type: 'directory', directory: '../lib' } },
    ['pkg@file:vendor/pkg.tgz' as DepPath]: { resolution: { integrity: 'sha512-pkg', tarball: 'file:vendor/pkg.tgz' } },
  },
}

test('createLocalDependenciesReader() resolves the local tarballs and directories of the wanted lockfile against its directory', async () => {
  const lockfileDir = await tempDir()
  const read = createLocalDependenciesReader(async (dir) => (dir === lockfileDir ? lockfile : null))
  expect(await read({ lockfileDir, current: false })).toStrictEqual({
    tarballs: [path.join(lockfileDir, 'vendor/pkg.tgz')],
    directories: [path.resolve(lockfileDir, '../lib')],
  })
  expect(await read({ lockfileDir: path.join(lockfileDir, 'other'), current: false })).toStrictEqual({})
})

test('createLocalDependenciesReader() reads the current lockfile when it stands in for the wanted one', async () => {
  const lockfileDir = await tempDir()
  await writeCurrentLockfile(path.join(lockfileDir, 'node_modules/.pnpm'), lockfile)
  const read = createLocalDependenciesReader(async () => {
    throw new Error('the wanted lockfile must not be read')
  })
  expect(await read({ lockfileDir, current: true })).toStrictEqual({
    tarballs: [path.join(lockfileDir, 'vendor/pkg.tgz')],
    directories: [path.resolve(lockfileDir, '../lib')],
  })
})
