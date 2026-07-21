import { existsSync } from 'node:fs'
import path from 'node:path'

import { beforeEach, expect, jest, test } from '@jest/globals'
import { WANTED_LOCKFILE } from '@pnpm/constants'
import type { InstallOptions } from '@pnpm/installing.deps-installer'
import type { LockfileFile, LockfileObject } from '@pnpm/lockfile.types'
import type { ResolveViaPnprServerOptions, ResolveViaPnprServerResult } from '@pnpm/pnpr.client'
import { prepareEmpty } from '@pnpm/prepare'
import type { ProjectId, ProjectManifest } from '@pnpm/types'

import { testDefaults } from '../utils/index.js'

const minimalLockfile: LockfileObject = {
  lockfileVersion: '9.0',
  importers: {
    ['.' as ProjectId]: { specifiers: {} },
  },
  packages: {},
}

const lockfileFs = await import('@pnpm/lockfile.fs')
const readWantedLockfileFile = jest.fn<typeof lockfileFs.readWantedLockfileFile>()

// The lockfile write stays real so tests can assert on the on-disk outcome
// instead of counting calls to an implementation detail.
jest.unstable_mockModule('@pnpm/lockfile.fs', () => ({
  ...lockfileFs,
  readWantedLockfileFile,
}))

const resolveViaPnprServer = jest.fn<
  (opts: ResolveViaPnprServerOptions) => Promise<ResolveViaPnprServerResult>
>()

jest.unstable_mockModule('@pnpm/pnpr.client', () => ({
  resolveViaPnprServer,
}))

const { install } = await import('@pnpm/installing.deps-installer')

beforeEach(() => {
  readWantedLockfileFile.mockReset()
  readWantedLockfileFile.mockResolvedValue(null)
  resolveViaPnprServer.mockReset()
  resolveViaPnprServer.mockResolvedValue({
    lockfile: minimalLockfile,
    stats: { totalPackages: 0 },
  })
})

test('pnpr forwards frozenLockfile when the lockfile is missing', async () => {
  await runInstall({ frozenLockfile: true })

  expect(resolveViaPnprServer).toHaveBeenCalledWith(expect.objectContaining({
    frozenLockfile: true,
    lockfile: undefined,
  }))
})

test('pnpr derives frozenLockfileIfExists without additional mutation after reading', async () => {
  const existingLockfile: LockfileFile = {
    lockfileVersion: '9.0',
    importers: {
      '.': {
        dependencies: {
          foo: { specifier: '1.0.0', version: '1.0.0' },
        },
      },
    },
    packages: {
      'foo@1.0.0': {
        resolution: { integrity: 'sha512-test' },
      },
    },
    snapshots: {
      'foo@1.0.0': {},
    },
  }
  const expectedLockfile = structuredClone(existingLockfile)
  readWantedLockfileFile.mockResolvedValueOnce(existingLockfile)

  await runInstall({
    manifest: { dependencies: { foo: '2.0.0' } },
    frozenLockfile: false,
    frozenLockfileIfExists: true,
  })

  expect(existingLockfile).toStrictEqual(expectedLockfile)
  expect(resolveViaPnprServer).toHaveBeenCalledWith(expect.objectContaining({
    frozenLockfile: true,
    lockfile: expectedLockfile,
  }))
})

test('pnpr does not freeze frozenLockfileIfExists when no lockfile exists', async () => {
  await runInstall({ frozenLockfileIfExists: true })

  expect(resolveViaPnprServer).toHaveBeenCalledWith(expect.objectContaining({
    frozenLockfile: false,
    lockfile: undefined,
  }))
})

test('pnpr does not freeze frozenLockfileIfExists for an empty lockfile', async () => {
  const emptyLockfile: LockfileFile = {
    lockfileVersion: '9.0',
    importers: { '.': {} },
  }
  readWantedLockfileFile.mockResolvedValueOnce(emptyLockfile)

  await runInstall({ frozenLockfileIfExists: true })

  expect(resolveViaPnprServer).toHaveBeenCalledWith(expect.objectContaining({
    frozenLockfile: false,
    lockfile: emptyLockfile,
  }))
})

test('pnpr forwards preferFrozenLockfile false and lockfile verification controls', async () => {
  await runInstall({
    preferFrozenLockfile: false,
    ignorePackageManifest: true,
    trustLockfile: true,
  })

  expect(resolveViaPnprServer).toHaveBeenCalledWith(expect.objectContaining({
    preferFrozenLockfile: false,
    ignoreManifestCheck: true,
    trustLockfile: true,
  }))
})

test.each([
  { useGitBranchLockfile: true, mergeGitBranchLockfiles: false },
  { useGitBranchLockfile: false, mergeGitBranchLockfiles: true },
  { useGitBranchLockfile: true, mergeGitBranchLockfiles: true },
])('pnpr forwards git-branch lockfile selection to the read ($useGitBranchLockfile, $mergeGitBranchLockfiles)', async ({
  useGitBranchLockfile,
  mergeGitBranchLockfiles,
}) => {
  await runInstall({
    useGitBranchLockfile,
    mergeGitBranchLockfiles,
    saveLockfile: false,
  })

  expect(readWantedLockfileFile).toHaveBeenCalledWith(expect.any(String), {
    ignoreIncompatible: true,
    useGitBranchLockfile,
    mergeGitBranchLockfiles,
  })
})

test.each([
  { useLockfile: false, saveLockfile: false, expectedReads: 0, writesLockfile: false },
  { useLockfile: false, saveLockfile: true, expectedReads: 0, writesLockfile: false },
  { useLockfile: true, saveLockfile: false, expectedReads: 1, writesLockfile: false },
  { useLockfile: true, saveLockfile: true, expectedReads: 1, writesLockfile: true },
])('pnpr honors lockfile I/O settings ($useLockfile, $saveLockfile)', async ({
  useLockfile,
  saveLockfile,
  expectedReads,
  writesLockfile,
}) => {
  const existingLockfile: LockfileFile = {
    lockfileVersion: '9.0',
    importers: { '.': {} },
  }
  readWantedLockfileFile.mockResolvedValueOnce(existingLockfile)

  await runInstall({ useLockfile, saveLockfile })

  expect(readWantedLockfileFile).toHaveBeenCalledTimes(expectedReads)
  expect(existsSync(path.join(process.cwd(), WANTED_LOCKFILE))).toBe(writesLockfile)
  expect(resolveViaPnprServer).toHaveBeenCalledWith(expect.objectContaining({
    lockfile: useLockfile ? existingLockfile : undefined,
  }))
})

async function runInstall (
  opts: Partial<InstallOptions> & { manifest?: ProjectManifest } = {}
): Promise<void> {
  prepareEmpty()
  const { manifest = {}, ...installOptions } = opts
  await install(manifest, testDefaults({
    pnprServer: 'http://pnpr.test',
    lockfileOnly: true,
    useLockfile: true,
    saveLockfile: true,
    frozenLockfile: false,
    frozenLockfileIfExists: false,
    ...installOptions,
  }))
}
