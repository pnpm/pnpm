import fs from 'node:fs'
import os from 'node:os'
import path from 'node:path'
import util from 'node:util'

import { beforeEach, expect, jest, test } from '@jest/globals'
import type { GlobalPackageInfo } from '@pnpm/global.packages'

const cleanOrphanedInstallDirs = jest.fn()
const createInstallDir = jest.fn()
const getHashLink = jest.fn()
const getInstalledBinNames = jest.fn<(pkg: GlobalPackageInfo) => Promise<string[]>>().mockResolvedValue([])
const scanGlobalPackages = jest.fn()
const checkGlobalBinConflicts = jest.fn<() => Promise<Set<string>>>().mockResolvedValue(new Set())
const installGlobalPackages = jest.fn<(...args: unknown[]) => Promise<{ ignoredBuilds: undefined, resolutionPolicyViolations: [] }>>()
  .mockResolvedValue({ ignoredBuilds: undefined, resolutionPolicyViolations: [] })
const promptApproveGlobalBuilds = jest.fn<() => Promise<void>>().mockResolvedValue(undefined)
const readInstalledPackages = jest.fn<() => Promise<[]>>().mockResolvedValue([])
const summaryDebug = jest.fn()
const activateGlobalInstall = jest.fn<(opts: unknown) => Promise<Set<string>>>().mockResolvedValue(new Set(['fresh']))
const cleanupReplacedGlobalInstalls = jest.fn<(opts: unknown) => Promise<void>>().mockResolvedValue(undefined)
const cleanupFailedGlobalInstall = jest.fn(async (installDir: string, originalError: unknown): Promise<never> => {
  try {
    await fs.promises.rm(installDir, { recursive: true, force: true })
  } catch (cleanupError) {
    throw new AggregateError(
      [originalError, cleanupError],
      'Failed to clean up after global install failed before activation.',
      { cause: originalError } // eslint-disable-line preserve-caught-error -- Matches the production contract: the failure before activation is primary.
    )
  }
  throw originalError
})

jest.unstable_mockModule('@pnpm/core-loggers', () => ({ summaryLogger: { debug: summaryDebug } }))
jest.unstable_mockModule('@pnpm/global.packages', () => ({
  cleanOrphanedInstallDirs,
  createInstallDir,
  getHashLink,
  getInstalledBinNames,
  scanGlobalPackages,
}))
jest.unstable_mockModule('../src/checkGlobalBinConflicts.js', () => ({ checkGlobalBinConflicts }))
jest.unstable_mockModule('../src/globalActivation.js', () => ({
  activateGlobalInstall,
  cleanupFailedGlobalInstall,
  cleanupReplacedGlobalInstalls,
}))
jest.unstable_mockModule('../src/installGlobalPackages.js', () => ({ installGlobalPackages }))
jest.unstable_mockModule('../src/promptApproveGlobalBuilds.js', () => ({ promptApproveGlobalBuilds }))
jest.unstable_mockModule('../src/readInstalledPackages.js', () => ({ readInstalledPackages }))

const { handleGlobalUpdate } = await import('../src/globalUpdate.js')

beforeEach(() => {
  jest.clearAllMocks()
  checkGlobalBinConflicts.mockResolvedValue(new Set())
  cleanupReplacedGlobalInstalls.mockResolvedValue(undefined)
  getInstalledBinNames.mockResolvedValue([])
  activateGlobalInstall.mockResolvedValue(new Set(['fresh']))
})

test('global update emits a single summary after updating all isolated groups', async () => {
  const updateResolutionPolicyManifest = jest.fn<() => Promise<void>>().mockResolvedValue(undefined)
  createInstallDir
    .mockReturnValueOnce('/global/v11/install-1')
    .mockReturnValueOnce('/global/v11/install-2')
  getHashLink
    .mockReturnValueOnce('/global/v11/hash-foo')
    .mockReturnValueOnce('/global/v11/hash-bar')
  const groups: GlobalPackageInfo[] = [
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
  ]
  scanGlobalPackages.mockReturnValue(groups)

  await handleGlobalUpdate({
    bin: '/global/bin',
    globalPkgDir: '/global/v11',
    updateResolutionPolicyManifest,
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
  expect(activateGlobalInstall).toHaveBeenNthCalledWith(1, {
    installDir: '/global/v11/install-1',
    hashLink: '/global/v11/hash-foo',
    globalBinDir: '/global/bin',
    pkgs: [],
    binsToSkip: new Set(),
  })
  expect(activateGlobalInstall).toHaveBeenNthCalledWith(2, {
    installDir: '/global/v11/install-2',
    hashLink: '/global/v11/hash-bar',
    globalBinDir: '/global/bin',
    pkgs: [],
    binsToSkip: new Set(),
  })
  expect(cleanupReplacedGlobalInstalls).toHaveBeenNthCalledWith(1, {
    groups: [{ info: groups[0], binNames: [] }],
    globalDir: '/global/v11',
    globalBinDir: '/global/bin',
    activeHash: 'hash-foo',
    activatedBins: new Set(['fresh']),
    protectedBins: new Set(),
  })
  expect(cleanupReplacedGlobalInstalls).toHaveBeenNthCalledWith(2, {
    groups: [{ info: groups[1], binNames: [] }],
    globalDir: '/global/v11',
    globalBinDir: '/global/bin',
    activeHash: 'hash-bar',
    activatedBins: new Set(['fresh']),
    protectedBins: new Set(),
  })
  const firstActivationOrder = activateGlobalInstall.mock.invocationCallOrder[0]
  for (const group of groups) {
    const ownershipCall = getInstalledBinNames.mock.calls.findIndex(([pkg]) => pkg === group)
    expect(ownershipCall).toBeGreaterThanOrEqual(0)
    expect(getInstalledBinNames.mock.invocationCallOrder[ownershipCall]).toBeLessThan(firstActivationOrder)
  }
  for (const index of [0, 1]) {
    expect(activateGlobalInstall.mock.invocationCallOrder[index]).toBeLessThan(cleanupReplacedGlobalInstalls.mock.invocationCallOrder[index])
    expect(cleanupReplacedGlobalInstalls.mock.invocationCallOrder[index]).toBeLessThan(updateResolutionPolicyManifest.mock.invocationCallOrder[index])
  }
  expect(summaryDebug).toHaveBeenCalledTimes(1)
  expect(summaryDebug).toHaveBeenCalledWith({ prefix: '/global/v11' })
})

test('global update removes the fresh install and does not activate when target ownership cannot be enumerated', async () => {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'global-update-ownership-'))
  expect(fs.realpathSync(path.dirname(root))).toBe(fs.realpathSync(os.tmpdir()))
  expect(root).not.toBe('')
  expect(fs.statSync(root).isDirectory()).toBe(true)
  const globalDir = path.join(root, 'global')
  const globalBinDir = path.join(root, 'bin')
  const oldInstallDir = path.join(globalDir, 'old-install')
  const freshInstallDir = path.join(globalDir, 'fresh-install')
  const oldMarker = path.join(oldInstallDir, 'marker')
  fs.mkdirSync(oldInstallDir, { recursive: true })
  fs.mkdirSync(freshInstallDir, { recursive: true })
  fs.mkdirSync(globalBinDir, { recursive: true })
  fs.writeFileSync(oldMarker, 'old install\n')
  fs.writeFileSync(path.join(freshInstallDir, 'marker'), 'fresh install\n')

  const target = {
    dependencies: { foo: '^1.0.0' },
    hash: 'hash-foo',
    installDir: oldInstallDir,
  }
  const survivor = {
    dependencies: { bar: '^2.0.0' },
    hash: 'hash-bar',
    installDir: path.join(globalDir, 'bar-install'),
  }
  const enumerationError = Object.assign(new Error('target package.json is missing'), { code: 'ENOENT' })
  createInstallDir.mockReturnValue(freshInstallDir)
  getHashLink.mockReturnValue(path.join(globalDir, target.hash))
  scanGlobalPackages.mockReturnValue([target, survivor])
  getInstalledBinNames.mockImplementation(async (pkg) => {
    if (pkg === target) throw enumerationError
    return ['bar']
  })

  try {
    let thrown: unknown
    try {
      await handleGlobalUpdate({
        bin: globalBinDir,
        globalPkgDir: globalDir,
      } as any, ['foo'], {}) // eslint-disable-line @typescript-eslint/no-explicit-any
    } catch (err) {
      thrown = err
    }

    const observed = {
      thrown,
      activated: activateGlobalInstall.mock.calls.length,
      cleanedUp: cleanupReplacedGlobalInstalls.mock.calls.length,
      oldMarker: fs.readFileSync(oldMarker, 'utf8'),
      freshInstallExists: fs.existsSync(freshInstallDir),
    }

    expect(observed).toMatchObject({
      thrown: enumerationError,
      activated: 0,
      cleanedUp: 0,
      oldMarker: 'old install\n',
      freshInstallExists: false,
    })
  } finally {
    expect(root).not.toBe('')
    expect(fs.existsSync(root)).toBe(true)
    expect(fs.statSync(root).isDirectory()).toBe(true)
    expect(fs.realpathSync(path.dirname(root))).toBe(fs.realpathSync(os.tmpdir()))
    fs.rmSync(root, { recursive: true, force: true })
  }
})

test('global update preserves ownership state and both errors when fresh install cleanup fails', async () => {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'global-update-cleanup-failure-'))
  expect(fs.realpathSync(path.dirname(root))).toBe(fs.realpathSync(os.tmpdir()))
  const globalDir = path.join(root, 'global')
  const globalBinDir = path.join(root, 'bin')
  const oldInstallDir = path.join(globalDir, 'old-install')
  const survivorInstallDir = path.join(globalDir, 'survivor-install')
  const freshInstallDir = path.join(globalDir, 'fresh-install')
  const oldHashLink = path.join(globalDir, 'hash-foo')
  const oldMarker = path.join(oldInstallDir, 'marker')
  const oldBin = path.join(globalBinDir, 'foo')
  fs.mkdirSync(oldInstallDir, { recursive: true })
  fs.mkdirSync(survivorInstallDir, { recursive: true })
  fs.mkdirSync(freshInstallDir, { recursive: true })
  fs.mkdirSync(globalBinDir, { recursive: true })
  fs.writeFileSync(oldMarker, 'old install\n')
  fs.writeFileSync(path.join(survivorInstallDir, 'marker'), 'survivor install\n')
  fs.writeFileSync(path.join(freshInstallDir, 'marker'), 'fresh install\n')
  fs.writeFileSync(oldBin, 'old foo shim\n')
  fs.symlinkSync(oldInstallDir, oldHashLink, process.platform === 'win32' ? 'junction' : 'dir')

  const target = {
    dependencies: { foo: '^1.0.0' },
    hash: 'hash-foo',
    installDir: oldInstallDir,
  }
  const survivor = {
    dependencies: { bar: '^2.0.0' },
    hash: 'hash-bar',
    installDir: survivorInstallDir,
  }
  const enumerationError = Object.assign(new Error('target package.json is missing'), { code: 'ENOENT' })
  const cleanupError = Object.assign(new Error('fresh install cleanup failed'), { code: 'EACCES' })
  createInstallDir.mockReturnValue(freshInstallDir)
  getHashLink.mockReturnValue(oldHashLink)
  scanGlobalPackages.mockReturnValue([target, survivor])
  getInstalledBinNames.mockImplementation(async (pkg) => {
    if (pkg === target) throw enumerationError
    return ['bar']
  })

  const snapshot = (): unknown => ({
    binEntries: fs.readdirSync(globalBinDir).sort(),
    globalEntries: fs.readdirSync(globalDir).sort(),
    oldBin: fs.readFileSync(oldBin, 'utf8'),
    oldHashTarget: fs.realpathSync(oldHashLink),
    oldMarker: fs.readFileSync(oldMarker, 'utf8'),
  })
  const before = snapshot()
  const realRm = fs.promises.rm.bind(fs.promises)
  const rmSpy = jest.spyOn(fs.promises, 'rm').mockImplementation(async (targetPath, options) => {
    if (path.resolve(String(targetPath)) === path.resolve(freshInstallDir)) throw cleanupError
    await realRm(targetPath, options)
  })

  try {
    let thrown: unknown
    try {
      await handleGlobalUpdate({
        bin: globalBinDir,
        globalPkgDir: globalDir,
      } as any, ['foo'], {}) // eslint-disable-line @typescript-eslint/no-explicit-any
    } catch (err) {
      thrown = err
    }

    expect(util.types.isNativeError(thrown)).toBe(true)
    const aggregateError = thrown as AggregateError
    expect(aggregateError.errors).toStrictEqual([enumerationError, cleanupError])
    expect(aggregateError.cause).toBe(enumerationError)
    expect(snapshot()).toStrictEqual(before)
    expect(rmSpy).toHaveBeenCalledWith(freshInstallDir, { recursive: true, force: true })
    expect(activateGlobalInstall).not.toHaveBeenCalled()
    expect(cleanupReplacedGlobalInstalls).not.toHaveBeenCalled()
  } finally {
    rmSpy.mockRestore()
    fs.rmSync(root, { recursive: true, force: true })
  }
})

test('global update only updates interactively selected groups', async () => {
  createInstallDir.mockReturnValue('/global/v11/install-1')
  getHashLink.mockReturnValue('/global/v11/hash-foo')
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
    selectedPackageHashes: new Set(['hash-foo']),
  } as any, [], {}) // eslint-disable-line @typescript-eslint/no-explicit-any

  expect(installGlobalPackages).toHaveBeenCalledTimes(1)
  expect(installGlobalPackages).toHaveBeenCalledWith(
    expect.objectContaining({ dir: '/global/v11/install-1' }),
    ['foo@^1.0.0']
  )
})

test('global update does not clean up or persist policy when activation fails', async () => {
  const group = {
    dependencies: { foo: '^1.0.0' },
    hash: 'hash-foo',
    installDir: '/global/v11/old-foo',
  }
  const activationError = new Error('activation failed')
  const updateResolutionPolicyManifest = jest.fn<() => Promise<void>>().mockResolvedValue(undefined)
  createInstallDir.mockReturnValue('/global/v11/install-1')
  getHashLink.mockReturnValue('/global/v11/hash-foo')
  scanGlobalPackages.mockReturnValue([group])
  activateGlobalInstall.mockRejectedValue(activationError)

  await expect(handleGlobalUpdate({
    bin: '/global/bin',
    globalPkgDir: '/global/v11',
    updateResolutionPolicyManifest,
  } as any, [], {})).rejects.toBe(activationError) // eslint-disable-line @typescript-eslint/no-explicit-any

  expect(cleanupReplacedGlobalInstalls).not.toHaveBeenCalled()
  expect(updateResolutionPolicyManifest).not.toHaveBeenCalled()
})

test('global update --latest drops the spec only of plain version dependencies', async () => {
  createInstallDir.mockReturnValueOnce('/global/v11/install-3')
  getHashLink.mockReturnValueOnce('/global/v11/hash-local')
  scanGlobalPackages.mockReturnValue([
    {
      dependencies: {
        'private-linked-pkg': 'link:/home/user/projects/private-linked-pkg',
        'local-tarball-pkg': 'file:/home/user/tarballs/local-tarball-pkg.tgz',
        'git-pkg': 'github:user/git-pkg',
        'remote-tarball-pkg': 'https://example.com/pkg.tgz',
        'aliased-pkg': 'npm:other-pkg@^2.0.0',
        'named-registry-pkg': 'gh:^3.0.0',
        foo: '^1.0.0',
        bar: 'next',
      },
      hash: 'hash-local',
      installDir: '/global/v11/old-local',
    },
  ])

  await handleGlobalUpdate({
    bin: '/global/bin',
    globalPkgDir: '/global/v11',
    latest: true,
  } as any, [], {}) // eslint-disable-line @typescript-eslint/no-explicit-any

  expect(installGlobalPackages).toHaveBeenCalledTimes(1)
  expect(installGlobalPackages).toHaveBeenCalledWith(
    expect.objectContaining({
      dir: '/global/v11/install-3',
      global: false,
      omitSummaryLog: true,
    }),
    [
      'private-linked-pkg@link:/home/user/projects/private-linked-pkg',
      'local-tarball-pkg@file:/home/user/tarballs/local-tarball-pkg.tgz',
      'git-pkg@github:user/git-pkg',
      'remote-tarball-pkg@https://example.com/pkg.tgz',
      'aliased-pkg@npm:other-pkg@^2.0.0',
      'named-registry-pkg@gh:^3.0.0',
      'foo',
      'bar',
    ]
  )
})
