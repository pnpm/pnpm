import path from 'node:path'

import { expect, test } from '@jest/globals'
import { headlessInstall } from '@pnpm/installing.deps-restorer'
import { type LockfileObject, readCurrentLockfile } from '@pnpm/lockfile.fs'
import { preparePackages } from '@pnpm/prepare'
import { getIntegrity } from '@pnpm/testing.registry-mock'
import type { DepPath, ProjectId } from '@pnpm/types'

import { testDefaults } from './utils/testDefaults.js'

test('retains installed packages when a selected importer is new to the current lockfile', async () => {
  preparePackages([
    { location: '.', package: { name: 'root', private: true } },
    { name: 'a', dependencies: { '@pnpm.e2e/foo': '100.0.0' } },
    { name: 'b', dependencies: { '@pnpm.e2e/bar': '100.0.0' } },
  ])
  const wantedLockfile: LockfileObject = {
    lockfileVersion: '9.0',
    importers: {
      ['.' as ProjectId]: { specifiers: {} },
      ['a' as ProjectId]: {
        specifiers: { '@pnpm.e2e/foo': '100.0.0' },
        dependencies: { '@pnpm.e2e/foo': '100.0.0' },
      },
    },
    packages: {
      ['@pnpm.e2e/foo@100.0.0' as DepPath]: {
        resolution: { integrity: getIntegrity('@pnpm.e2e/foo', '100.0.0') },
      },
    },
  }
  const opts = await testDefaults({
    projects: [path.resolve('a'), path.resolve('b')],
    selectedProjectDirs: [path.resolve('a')],
    wantedLockfile,
  })
  await headlessInstall(opts)
  const before = await readCurrentLockfile('node_modules/.pnpm', { ignoreIncompatible: false })
  expect(before?.importers).not.toHaveProperty('b')

  wantedLockfile.importers['b' as ProjectId] = {
    specifiers: { '@pnpm.e2e/bar': '100.0.0' },
    dependencies: { '@pnpm.e2e/bar': '100.0.0' },
  }
  wantedLockfile.packages!['@pnpm.e2e/bar@100.0.0' as DepPath] = {
    resolution: { integrity: getIntegrity('@pnpm.e2e/bar', '100.0.0') },
  }
  await headlessInstall({ ...opts, selectedProjectDirs: [path.resolve('b')] })
  const current = await readCurrentLockfile('node_modules/.pnpm', { ignoreIncompatible: false })
  expect(current?.importers).toHaveProperty(['b', 'dependencies', '@pnpm.e2e/bar'], '100.0.0')
  expect(Object.keys(current!.packages!).sort()).toStrictEqual([
    '@pnpm.e2e/bar@100.0.0',
    '@pnpm.e2e/foo@100.0.0',
  ])
})

test('does not record dependencies for unselected importers in the current lockfile', async () => {
  preparePackages([
    { location: '.', package: { name: 'root', private: true, dependencies: { '@pnpm.e2e/foo': '100.0.0' } } },
    { name: 'a', dependencies: { '@pnpm.e2e/foo': '100.0.0' } },
  ])
  const wantedLockfile: LockfileObject = {
    lockfileVersion: '9.0',
    importers: {
      ['.' as ProjectId]: {
        specifiers: { '@pnpm.e2e/foo': '100.0.0' },
        dependencies: { '@pnpm.e2e/foo': '100.0.0' },
      },
      ['a' as ProjectId]: {
        specifiers: { '@pnpm.e2e/foo': '100.0.0' },
        dependencies: { '@pnpm.e2e/foo': '100.0.0' },
      },
    },
    packages: {
      ['@pnpm.e2e/foo@100.0.0' as DepPath]: {
        resolution: { integrity: getIntegrity('@pnpm.e2e/foo', '100.0.0') },
      },
    },
  }
  const opts = await testDefaults({
    projects: [path.resolve('.'), path.resolve('a')],
    selectedProjectDirs: [path.resolve('a')],
    wantedLockfile,
  })
  await headlessInstall(opts)
  const current = await readCurrentLockfile('node_modules/.pnpm', { ignoreIncompatible: false })
  expect(current?.importers).toEqual({
    '.': { specifiers: {} },
    a: {
      specifiers: { '@pnpm.e2e/foo': '100.0.0' },
      dependencies: { '@pnpm.e2e/foo': '100.0.0' },
    },
  })
})
