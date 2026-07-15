import fs from 'node:fs'

import { expect, jest, test } from '@jest/globals'

const linkBinsOfPackages = jest.fn<() => Promise<void>>().mockResolvedValue(undefined)
const removeBin = jest.fn<() => Promise<void>>().mockResolvedValue(undefined)
const cleanOrphanedInstallDirs = jest.fn()
const createInstallDir = jest.fn()
const getHashLink = jest.fn()
const getInstalledBinNames = jest.fn<() => Promise<string[]>>().mockResolvedValue([])
const scanGlobalPackages = jest.fn()
const checkGlobalBinConflicts = jest.fn<() => Promise<Set<string>>>().mockResolvedValue(new Set())
const installGlobalPackages = jest.fn<(...args: unknown[]) => Promise<{ ignoredBuilds: undefined, resolutionPolicyViolations: [] }>>()
  .mockResolvedValue({ ignoredBuilds: undefined, resolutionPolicyViolations: [] })
const promptApproveGlobalBuilds = jest.fn<() => Promise<void>>().mockResolvedValue(undefined)
const readInstalledPackages = jest.fn<() => Promise<[]>>().mockResolvedValue([])
const summaryDebug = jest.fn()
const symlinkDir = jest.fn<() => Promise<void>>().mockResolvedValue(undefined)

jest.unstable_mockModule('@pnpm/bins.linker', () => ({ linkBinsOfPackages }))
jest.unstable_mockModule('@pnpm/bins.remover', () => ({ removeBin }))
jest.unstable_mockModule('@pnpm/core-loggers', () => ({ summaryLogger: { debug: summaryDebug } }))
jest.unstable_mockModule('@pnpm/global.packages', () => ({
  cleanOrphanedInstallDirs,
  createInstallDir,
  getHashLink,
  getInstalledBinNames,
  scanGlobalPackages,
}))
jest.unstable_mockModule('is-subdir', () => ({ isSubdir: () => false }))
jest.unstable_mockModule('symlink-dir', () => ({ symlinkDir }))
jest.unstable_mockModule('../src/checkGlobalBinConflicts.js', () => ({ checkGlobalBinConflicts }))
jest.unstable_mockModule('../src/installGlobalPackages.js', () => ({ installGlobalPackages }))
jest.unstable_mockModule('../src/promptApproveGlobalBuilds.js', () => ({ promptApproveGlobalBuilds }))
jest.unstable_mockModule('../src/readInstalledPackages.js', () => ({ readInstalledPackages }))

const { handleGlobalUpdate } = await import('../src/globalUpdate.js')

test('global update emits a single summary after updating all isolated groups', async () => {
  createInstallDir
    .mockReturnValueOnce('/global/v11/install-1')
    .mockReturnValueOnce('/global/v11/install-2')
  getHashLink
    .mockReturnValueOnce('/global/v11/hash-foo')
    .mockReturnValueOnce('/global/v11/hash-bar')
  scanGlobalPackages.mockReturnValue([
    {
      dependencies: { foo: '^1.0.0' },
      hash: 'hash-foo',
      installDir: '/global/v11/old-foo',
    },
    {
      dependencies: { bar: '^2.0.0' },
      hash: 'hash-bar',
      installDir: '/global/v11/old-bar',
    },
  ])

  await handleGlobalUpdate({
    bin: '/global/bin',
    globalPkgDir: '/global/v11',
  } as any, [], {}) // eslint-disable-line @typescript-eslint/no-explicit-any

  expect(installGlobalPackages).toHaveBeenCalledTimes(2)
  expect(installGlobalPackages).toHaveBeenNthCalledWith(
    1,
    expect.objectContaining({
      dir: '/global/v11/install-1',
      global: false,
      omitSummaryLog: true,
    }),
    ['foo@^1.0.0']
  )
  expect(installGlobalPackages).toHaveBeenNthCalledWith(
    2,
    expect.objectContaining({
      dir: '/global/v11/install-2',
      global: false,
      omitSummaryLog: true,
    }),
    ['bar@^2.0.0']
  )
  expect(summaryDebug).toHaveBeenCalledTimes(1)
  expect(summaryDebug).toHaveBeenCalledWith({ prefix: '/global/v11' })
})

test('global update invokes checkLicensesAfterGlobalInstall once per package group, with that group\'s installDir', async () => {
  createInstallDir
    .mockReturnValueOnce('/global/v11/install-foo')
    .mockReturnValueOnce('/global/v11/install-bar')
  getHashLink
    .mockReturnValueOnce('/global/v11/hash-foo')
    .mockReturnValueOnce('/global/v11/hash-bar')
  scanGlobalPackages.mockReturnValue([
    {
      dependencies: { foo: '^1.0.0' },
      hash: 'hash-foo',
      installDir: '/global/v11/old-foo',
    },
    {
      dependencies: { bar: '^2.0.0' },
      hash: 'hash-bar',
      installDir: '/global/v11/old-bar',
    },
  ])
  const checkLicensesAfterGlobalInstall = jest.fn<(installDir: string) => Promise<void>>().mockResolvedValue(undefined)

  await handleGlobalUpdate({
    bin: '/global/bin',
    globalPkgDir: '/global/v11',
    checkLicensesAfterGlobalInstall,
  } as any, [], {}) // eslint-disable-line @typescript-eslint/no-explicit-any

  expect(checkLicensesAfterGlobalInstall).toHaveBeenCalledTimes(2)
  expect(checkLicensesAfterGlobalInstall).toHaveBeenNthCalledWith(1, '/global/v11/install-foo')
  expect(checkLicensesAfterGlobalInstall).toHaveBeenNthCalledWith(2, '/global/v11/install-bar')
})

test('global update removes the group install dir and rethrows when checkLicensesAfterGlobalInstall throws', async () => {
  createInstallDir.mockReturnValueOnce('/global/v11/install-violating')
  scanGlobalPackages.mockReturnValue([
    {
      dependencies: { foo: '^1.0.0' },
      hash: 'hash-foo',
      installDir: '/global/v11/old-foo',
    },
  ])
  // These are asserted as not-called below; clear prior tests' call history
  // (this file has no shared beforeEach reset) so the assertions reflect
  // only what happens during this test.
  checkGlobalBinConflicts.mockClear()
  linkBinsOfPackages.mockClear()
  const removeDirSpy = jest.spyOn(fs.promises, 'rm').mockResolvedValue(undefined)
  const violation = new Error('license violation')
  const checkLicensesAfterGlobalInstall = jest.fn<(installDir: string) => Promise<void>>().mockRejectedValue(violation)

  await expect(handleGlobalUpdate({
    bin: '/global/bin',
    globalPkgDir: '/global/v11',
    checkLicensesAfterGlobalInstall,
  } as any, [], {})).rejects.toBe(violation) // eslint-disable-line @typescript-eslint/no-explicit-any

  expect(checkLicensesAfterGlobalInstall).toHaveBeenCalledWith('/global/v11/install-violating')
  expect(removeDirSpy).toHaveBeenCalledWith('/global/v11/install-violating', { recursive: true, force: true })
  // The failure must abort before bin conflicts are checked or bins are linked,
  // so the violating group isn't left half-applied.
  expect(checkGlobalBinConflicts).not.toHaveBeenCalled()
  expect(linkBinsOfPackages).not.toHaveBeenCalled()

  removeDirSpy.mockRestore()
})
