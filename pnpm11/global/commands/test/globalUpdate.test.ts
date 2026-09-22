import fs from 'node:fs'
import os from 'node:os'
import path from 'node:path'
import util from 'node:util'

import { beforeEach, expect, jest, test } from '@jest/globals'
import type { GlobalPackageInfo } from '@pnpm/global.packages'

const cleanOrphanedInstallDirs = jest.fn()
const createInstallDir = jest.fn()
const getHashLink = jest.fn()
const getGlobalPackageDetails = jest.fn<(pkg: unknown) => Promise<Array<{ alias: string, version: string }>>>().mockResolvedValue([])
const getInstalledBinNames = jest.fn<(pkg: GlobalPackageInfo) => Promise<string[]>>().mockResolvedValue([])
const readModulesManifest = jest.fn<() => Promise<{ ignoredBuilds?: Set<string> } | null>>().mockResolvedValue(null)
const readWantedLockfile = jest.fn<() => Promise<unknown>>().mockResolvedValue(null)
const scanGlobalPackages = jest.fn()
const checkGlobalBinConflicts = jest.fn<() => Promise<Set<string>>>().mockResolvedValue(new Set())
const installGlobalPackages = jest.fn<(...args: unknown[]) => Promise<{ ignoredBuilds: undefined, resolutionPolicyViolations: Array<{ name: string, version: string, code: string, reason: string }>, resolvedVersions: Record<string, string> }>>()
  .mockResolvedValue({ ignoredBuilds: undefined, resolutionPolicyViolations: [], resolvedVersions: {} })
const promptApproveGlobalBuilds = jest.fn<(...args: unknown[]) => Promise<void>>().mockResolvedValue(undefined)
const readInstalledPackages = jest.fn<(installDir: string) => Promise<Array<{ alias: string, manifest: { name: string, version: string } }>>>().mockResolvedValue([])
const summaryDebug = jest.fn()
const info = jest.fn()
const activateGlobalInstall = jest.fn<(opts: unknown) => Promise<Set<string>>>().mockResolvedValue(new Set(['fresh']))
const cleanupReplacedGlobalInstalls = jest.fn<(opts: unknown) => Promise<void>>().mockResolvedValue(undefined)
const getActualBinNames = jest.fn<(opts: unknown) => Promise<Set<string>>>().mockResolvedValue(new Set(['fresh']))

jest.unstable_mockModule('@pnpm/core-loggers', () => ({ summaryLogger: { debug: summaryDebug } }))
jest.unstable_mockModule('@pnpm/global.packages', () => ({
  cleanOrphanedInstallDirs,
  createInstallDir,
  getGlobalPackageDetails,
  getHashLink,
  getInstalledBinNames,
  scanGlobalPackages,
}))
jest.unstable_mockModule('@pnpm/installing.modules-yaml', () => ({ readModulesManifest }))
jest.unstable_mockModule('@pnpm/lockfile.fs', () => ({ readWantedLockfile }))
jest.unstable_mockModule('@pnpm/logger', () => ({ logger: { info } }))
jest.unstable_mockModule('../src/checkGlobalBinConflicts.js', () => ({ checkGlobalBinConflicts }))
jest.unstable_mockModule('../src/globalActivation.js', () => ({
  activateGlobalInstall,
  cleanupReplacedGlobalInstalls,
  getActualBinNames,
}))
jest.unstable_mockModule('../src/installGlobalPackages.js', () => ({ installGlobalPackages }))
jest.unstable_mockModule('../src/promptApproveGlobalBuilds.js', () => ({ promptApproveGlobalBuilds }))
jest.unstable_mockModule('../src/readInstalledPackages.js', () => ({ readInstalledPackages }))

const { handleGlobalUpdate } = await import('../src/globalUpdate.js')

beforeEach(() => {
  jest.clearAllMocks()
  checkGlobalBinConflicts.mockResolvedValue(new Set())
  cleanupReplacedGlobalInstalls.mockResolvedValue(undefined)
  getGlobalPackageDetails.mockResolvedValue([])
  getInstalledBinNames.mockResolvedValue([])
  readModulesManifest.mockResolvedValue(null)
  readWantedLockfile.mockResolvedValue(null)
  installGlobalPackages.mockResolvedValue({
    ignoredBuilds: undefined,
    resolutionPolicyViolations: [],
    resolvedVersions: {},
  })
  readInstalledPackages.mockResolvedValue([])
  activateGlobalInstall.mockResolvedValue(new Set(['fresh']))
  getActualBinNames.mockResolvedValue(new Set(['fresh']))
})

test('global update emits a single summary after updating all isolated groups', async () => {
  const updateResolutionPolicyManifest = jest.fn<(violations: unknown[], dir: string) => Promise<void>>().mockResolvedValue(undefined)
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

  expect(installGlobalPackages).toHaveBeenCalledTimes(4)
  expect(installGlobalPackages).toHaveBeenNthCalledWith(
    1,
    expect.objectContaining({
      dir: '/global/v11/install-1',
      global: false,
      lockfileOnly: true,
      omitSummaryLog: true,
      rootProjectManifest: { dependencies: { foo: '^1.0.0' } },
    }),
    ['foo@^1.0.0']
  )
  expect(installGlobalPackages).toHaveBeenNthCalledWith(
    3,
    expect.objectContaining({
      dir: '/global/v11/install-2',
      global: false,
      lockfileOnly: true,
      omitSummaryLog: true,
      rootProjectManifest: { dependencies: { bar: '^2.0.0' } },
    }),
    ['bar@^2.0.0']
  )
  expect(activateGlobalInstall).toHaveBeenNthCalledWith(1, {
    installDir: '/global/v11/install-1',
    hashLink: '/global/v11/hash-foo',
    globalBinDir: '/global/bin',
    pkgs: [],
    binsToSkip: new Set(),
    requiredBinNames: new Set(['fresh']),
  })
  expect(activateGlobalInstall).toHaveBeenNthCalledWith(2, {
    installDir: '/global/v11/install-2',
    hashLink: '/global/v11/hash-bar',
    globalBinDir: '/global/bin',
    pkgs: [],
    binsToSkip: new Set(),
    requiredBinNames: new Set(['fresh']),
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
  for (const [index, group] of groups.entries()) {
    const ownershipCall = getInstalledBinNames.mock.calls.findIndex(([pkg]) => pkg === group)
    expect(ownershipCall).toBeGreaterThanOrEqual(0)
    expect(getInstalledBinNames.mock.invocationCallOrder[ownershipCall]).toBeLessThan(activateGlobalInstall.mock.invocationCallOrder[index])
  }
  for (const index of [0, 1]) {
    expect(activateGlobalInstall.mock.invocationCallOrder[index]).toBeLessThan(cleanupReplacedGlobalInstalls.mock.invocationCallOrder[index])
    expect(cleanupReplacedGlobalInstalls.mock.invocationCallOrder[index]).toBeLessThan(updateResolutionPolicyManifest.mock.invocationCallOrder[index])
  }
  expect(summaryDebug).toHaveBeenCalledTimes(1)
  expect(summaryDebug).toHaveBeenCalledWith({ prefix: '/global/v11' })
})

test('global update reports already up to date without replacing an equal candidate', async () => {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'global-update-unchanged-'))
  const globalDir = path.join(root, 'global')
  const oldInstallDir = path.join(globalDir, 'old-install')
  const candidateDir = path.join(globalDir, 'candidate')
  fs.mkdirSync(oldInstallDir, { recursive: true })
  fs.mkdirSync(candidateDir, { recursive: true })
  createInstallDir.mockReturnValue(candidateDir)
  scanGlobalPackages.mockReturnValue([
    { dependencies: { foo: '^1.0.0' }, hash: 'hash-foo', installDir: oldInstallDir },
  ])
  const lockfile = { importers: { '.': { dependencies: { foo: '1.0.0' } } }, lockfileVersion: '9.0' }
  readWantedLockfile.mockResolvedValue(lockfile)
  const violation = { name: 'foo', version: '1.0.0', code: 'policy', reason: 'test' }
  const ignoredBuilds = new Set(['foo@1.0.0'])
  readModulesManifest.mockResolvedValue({ ignoredBuilds })
  installGlobalPackages.mockResolvedValue({
    ignoredBuilds: undefined,
    resolutionPolicyViolations: [violation],
    resolvedVersions: { foo: '1.0.0' },
  })
  const updateResolutionPolicyManifest = jest.fn<(violations: unknown[], dir: string) => Promise<void>>().mockResolvedValue(undefined)

  try {
    const output = await handleGlobalUpdate({
      dir: root,
      bin: path.join(root, 'bin'),
      globalPkgDir: globalDir,
      updateResolutionPolicyManifest,
    } as any, [], {}) // eslint-disable-line @typescript-eslint/no-explicit-any

    expect(output).toBeUndefined()
    expect(info).toHaveBeenCalledWith({ message: 'Already up to date', prefix: root })
    expect(installGlobalPackages).toHaveBeenCalledTimes(1)
    expect(installGlobalPackages).toHaveBeenCalledWith(
      expect.objectContaining({ lockfileOnly: true, rootProjectManifest: { dependencies: { foo: '^1.0.0' } } }),
      ['foo@^1.0.0']
    )
    expect(fs.existsSync(candidateDir)).toBe(false)
    expect(fs.existsSync(oldInstallDir)).toBe(true)
    expect(activateGlobalInstall).not.toHaveBeenCalled()
    expect(promptApproveGlobalBuilds).toHaveBeenCalledWith({
      globalPkgDir: globalDir,
      installDir: oldInstallDir,
      ignoredBuilds,
      allowBuilds: {},
      inheritedOpts: expect.objectContaining({ dir: root }),
    }, {})
    expect(updateResolutionPolicyManifest).toHaveBeenCalledWith([violation], globalDir)
    expect(summaryDebug).toHaveBeenCalledTimes(1)
    expect(summaryDebug).toHaveBeenCalledWith({ prefix: globalDir })
  } finally {
    fs.rmSync(root, { recursive: true, force: true })
  }
})

test('global update does not report already up to date when the active group lost its node_modules', async () => {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'global-update-no-modules-'))
  const globalDir = path.join(root, 'global')
  const oldInstallDir = path.join(globalDir, 'old-install')
  const candidateDir = path.join(globalDir, 'candidate')
  fs.mkdirSync(oldInstallDir, { recursive: true })
  fs.mkdirSync(candidateDir, { recursive: true })
  createInstallDir.mockReturnValue(candidateDir)
  getHashLink.mockReturnValue(path.join(globalDir, 'hash-foo'))
  scanGlobalPackages.mockReturnValue([
    { dependencies: { foo: '^1.0.0' }, hash: 'hash-foo', installDir: oldInstallDir },
  ])
  readWantedLockfile.mockResolvedValue({ importers: { '.': { dependencies: { foo: '1.0.0' } } }, lockfileVersion: '9.0' })
  readModulesManifest.mockResolvedValue(null)

  try {
    await handleGlobalUpdate({
      dir: root,
      bin: path.join(root, 'bin'),
      globalPkgDir: globalDir,
    } as any, [], {}) // eslint-disable-line @typescript-eslint/no-explicit-any

    expect(info).not.toHaveBeenCalled()
    expect(installGlobalPackages).toHaveBeenCalledTimes(2)
    expect(installGlobalPackages).toHaveBeenNthCalledWith(
      2,
      expect.objectContaining({ dir: candidateDir, lockfileOnly: false }),
      ['foo@^1.0.0']
    )
    expect(fs.existsSync(candidateDir)).toBe(true)
    expect(activateGlobalInstall).toHaveBeenCalled()
  } finally {
    fs.rmSync(root, { recursive: true, force: true })
  }
})

test('global update fails without replacing the active group when equal-candidate cleanup fails', async () => {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'global-update-unchanged-cleanup-'))
  const globalDir = path.join(root, 'global')
  const oldInstallDir = path.join(globalDir, 'old-install')
  const candidateDir = path.join(globalDir, 'candidate')
  const oldMarker = path.join(oldInstallDir, 'marker')
  fs.mkdirSync(oldInstallDir, { recursive: true })
  fs.mkdirSync(candidateDir, { recursive: true })
  fs.writeFileSync(oldMarker, 'active\n')
  createInstallDir.mockReturnValue(candidateDir)
  scanGlobalPackages.mockReturnValue([
    { dependencies: { foo: '^1.0.0' }, hash: 'hash-foo', installDir: oldInstallDir },
  ])
  readWantedLockfile.mockResolvedValue({ importers: {}, lockfileVersion: '9.0' })
  readModulesManifest.mockResolvedValue({})
  const cleanupError = Object.assign(new Error('candidate cleanup failed'), { code: 'EACCES' })
  const realRm = fs.promises.rm.bind(fs.promises)
  const rmSpy = jest.spyOn(fs.promises, 'rm').mockImplementation(async (targetPath, options) => {
    if (path.resolve(String(targetPath)) === path.resolve(candidateDir)) throw cleanupError
    await realRm(targetPath, options)
  })

  try {
    await expect(handleGlobalUpdate({
      bin: path.join(root, 'bin'),
      globalPkgDir: globalDir,
    } as any, [], {})).rejects.toBe(cleanupError) // eslint-disable-line @typescript-eslint/no-explicit-any
    expect(fs.readFileSync(oldMarker, 'utf8')).toBe('active\n')
    expect(activateGlobalInstall).not.toHaveBeenCalled()
    expect(cleanupReplacedGlobalInstalls).not.toHaveBeenCalled()
  } finally {
    rmSpy.mockRestore()
    fs.rmSync(root, { recursive: true, force: true })
  }
})

test('global update ignores incomplete survivors when every replaced bin is retained', async () => {
  const target: GlobalPackageInfo = {
    dependencies: { foo: '^1.0.0' },
    hash: 'hash-foo',
    installDir: '/global/v11/old-foo',
  }
  const survivor: GlobalPackageInfo = {
    dependencies: { bar: '^2.0.0' },
    hash: 'hash-bar',
    installDir: '/global/v11/old-bar',
  }
  const survivorError = Object.assign(new Error('survivor package.json is missing'), { code: 'ENOENT' })
  createInstallDir.mockReturnValue('/global/v11/install-1')
  getHashLink.mockReturnValue('/global/v11/hash-foo')
  scanGlobalPackages.mockReturnValue([target, survivor])
  getInstalledBinNames.mockImplementation(async (pkg) => {
    if (pkg === survivor) throw survivorError
    return ['fresh']
  })

  await handleGlobalUpdate({
    bin: '/global/bin',
    globalPkgDir: '/global/v11',
  } as any, ['foo'], {}) // eslint-disable-line @typescript-eslint/no-explicit-any

  expect(getInstalledBinNames).toHaveBeenCalledTimes(1)
  expect(getInstalledBinNames).toHaveBeenCalledWith(target)
  expect(activateGlobalInstall).toHaveBeenCalledWith(expect.objectContaining({
    requiredBinNames: new Set(['fresh']),
  }))
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

  expect(installGlobalPackages).toHaveBeenCalledTimes(2)
  expect(installGlobalPackages).toHaveBeenNthCalledWith(
    1,
    expect.objectContaining({ dir: '/global/v11/install-1', lockfileOnly: true }),
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
  const updateResolutionPolicyManifest = jest.fn<(violations: unknown[], dir: string) => Promise<void>>().mockResolvedValue(undefined)
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

  expect(installGlobalPackages).toHaveBeenCalledTimes(2)
  expect(installGlobalPackages).toHaveBeenNthCalledWith(
    1,
    expect.objectContaining({
      dir: '/global/v11/install-3',
      global: false,
      lockfileOnly: true,
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

// `pnpm self-update` owns the pnpm CLI's global install: it is what points the
// pnpm home's bins at a release. Updating that group here would resolve pnpm
// from the `latest` dist-tag and relink the bins, silently rolling the running
// pnpm back to whatever `latest` points at (pnpm/pnpm#14270).
test('global update leaves the pnpm CLI to self-update', async () => {
  createInstallDir.mockReturnValueOnce('/global/v11/install-1')
  getHashLink.mockReturnValueOnce('/global/v11/hash-foo')
  scanGlobalPackages.mockReturnValue([
    {
      dependencies: { pnpm: '12.0.0' },
      hash: 'hash-pnpm',
      installDir: '/global/v11/old-pnpm',
    },
    {
      dependencies: { '@pnpm/exe': '11.24.0' },
      hash: 'hash-exe',
      installDir: '/global/v11/old-exe',
    },
    {
      dependencies: { foo: '^1.0.0' },
      hash: 'hash-foo',
      installDir: '/global/v11/old-foo',
    },
  ])

  await handleGlobalUpdate({
    bin: '/global/bin',
    globalPkgDir: '/global/v11',
    latest: true,
  } as any, [], {}) // eslint-disable-line @typescript-eslint/no-explicit-any

  expect(installGlobalPackages).toHaveBeenCalledTimes(2)
  expect(installGlobalPackages).toHaveBeenNthCalledWith(
    1,
    expect.objectContaining({ dir: '/global/v11/install-1', lockfileOnly: true }),
    ['foo']
  )
})

test('global update reports nothing to do when only the pnpm CLI is installed globally', async () => {
  scanGlobalPackages.mockReturnValue([
    {
      dependencies: { '@pnpm/exe': '11.24.0' },
      hash: 'hash-exe',
      installDir: '/global/v11/old-exe',
    },
  ])

  const output = await handleGlobalUpdate({
    bin: '/global/bin',
    globalPkgDir: '/global/v11',
  } as any, [], {}) // eslint-disable-line @typescript-eslint/no-explicit-any

  expect(output).toBe('No global packages to update. Run "pnpm self-update" to update pnpm itself.')
  expect(installGlobalPackages).not.toHaveBeenCalled()
})

// `--latest` resolves the `latest` dist-tag, which points at an older release
// than the one installed whenever that came from another tag or from a major
// that has not been promoted yet. An update must never move a package
// backwards, so the group is reinstalled holding it where it is
// (pnpm/pnpm#14270).
test('global update --latest holds a package that latest would downgrade', async () => {
  createInstallDir.mockReturnValueOnce('/global/v11/install-1')
  getHashLink.mockReturnValue('/global/v11/hash-mixed')
  scanGlobalPackages.mockReturnValue([
    {
      dependencies: { prerelease: '^2.0.0', stable: '^1.0.0' },
      hash: 'hash-mixed',
      installDir: '/global/v11/old-mixed',
    },
  ])
  getGlobalPackageDetails.mockResolvedValue([
    { alias: 'prerelease', version: '2.0.0' },
    { alias: 'stable', version: '1.0.0' },
  ])
  installGlobalPackages.mockResolvedValue({
    ignoredBuilds: undefined,
    resolutionPolicyViolations: [],
    resolvedVersions: { prerelease: '1.9.0', stable: '1.2.0' },
  })

  await handleGlobalUpdate({
    bin: '/global/bin',
    globalPkgDir: '/global/v11',
    latest: true,
  } as any, [], {}) // eslint-disable-line @typescript-eslint/no-explicit-any

  // The probe resolves without installing, so a rejected release never gets to
  // run its lifecycle scripts. It resolves into the group's own directory, so
  // the install that follows reuses the lockfile it wrote.
  expect(installGlobalPackages).toHaveBeenNthCalledWith(
    1,
    expect.objectContaining({ dir: '/global/v11/install-1', lockfileOnly: true }),
    ['prerelease', 'stable']
  )
  // Only the one that went backwards is held; the other keeps its update.
  expect(installGlobalPackages).toHaveBeenNthCalledWith(
    2,
    expect.objectContaining({ dir: '/global/v11/install-1', lockfileOnly: true }),
    ['prerelease@2.0.0', 'stable']
  )
  expect(installGlobalPackages).toHaveBeenNthCalledWith(
    3,
    expect.objectContaining({ dir: '/global/v11/install-1', lockfileOnly: false }),
    ['prerelease@2.0.0', 'stable']
  )
  expect(activateGlobalInstall).toHaveBeenCalledWith(
    expect.objectContaining({ installDir: '/global/v11/install-1' })
  )
})

test('global update without --latest resolves once before materialization', async () => {
  createInstallDir.mockReturnValueOnce('/global/v11/install-1')
  getHashLink.mockReturnValue('/global/v11/hash-foo')
  scanGlobalPackages.mockReturnValue([
    { dependencies: { foo: '^1.0.0' }, hash: 'hash-foo', installDir: '/global/v11/old-foo' },
  ])
  getGlobalPackageDetails.mockResolvedValue([{ alias: 'foo', version: '1.0.0' }])

  await handleGlobalUpdate({
    bin: '/global/bin',
    globalPkgDir: '/global/v11',
  } as any, [], {}) // eslint-disable-line @typescript-eslint/no-explicit-any

  expect(installGlobalPackages).toHaveBeenCalledTimes(2)
  expect(installGlobalPackages).toHaveBeenNthCalledWith(
    1,
    expect.objectContaining({ dir: '/global/v11/install-1', lockfileOnly: true }),
    ['foo@^1.0.0']
  )
  expect(installGlobalPackages).toHaveBeenNthCalledWith(
    2,
    expect.objectContaining({ dir: '/global/v11/install-1', lockfileOnly: false }),
    ['foo@^1.0.0']
  )
})

test('global update approves an immature version once across its resolution passes', async () => {
  createInstallDir.mockReturnValue('/global/v11/install-1')
  getHashLink.mockReturnValue('/global/v11/hash-foo')
  scanGlobalPackages.mockReturnValue([
    { dependencies: { foo: '^1.0.0' }, hash: 'hash-foo', installDir: '/global/v11/old-foo' },
  ])
  const violation = { name: 'foo', version: '2.0.0', code: 'MINIMUM_RELEASE_AGE_VIOLATION', reason: 'published too recently' }
  installGlobalPackages.mockImplementation(async (...args: unknown[]) => {
    const installOpts = args[0] as { handleResolutionPolicyViolations?: (violations: unknown[]) => Promise<void> }
    await installOpts.handleResolutionPolicyViolations?.([violation])
    return { ignoredBuilds: undefined, resolutionPolicyViolations: [violation], resolvedVersions: { foo: '2.0.0' } }
  })
  const handleResolutionPolicyViolations = jest.fn<(violations: unknown[]) => Promise<void>>().mockResolvedValue(undefined)

  await handleGlobalUpdate({
    bin: '/global/bin',
    globalPkgDir: '/global/v11',
    handleResolutionPolicyViolations,
  } as any, [], {}) // eslint-disable-line @typescript-eslint/no-explicit-any

  expect(installGlobalPackages).toHaveBeenCalledTimes(2)
  expect(handleResolutionPolicyViolations).toHaveBeenCalledTimes(1)
  expect(handleResolutionPolicyViolations).toHaveBeenCalledWith([violation])
  expect(activateGlobalInstall).toHaveBeenCalledTimes(1)
})

test('global update approves a version only a later resolution pass reports', async () => {
  createInstallDir.mockReturnValue('/global/v11/install-1')
  getHashLink.mockReturnValue('/global/v11/hash-foo')
  scanGlobalPackages.mockReturnValue([
    { dependencies: { foo: '^1.0.0' }, hash: 'hash-foo', installDir: '/global/v11/old-foo' },
  ])
  const first = { name: 'foo', version: '2.0.0', code: 'MINIMUM_RELEASE_AGE_VIOLATION', reason: 'published too recently' }
  const later = { name: 'bar', version: '3.0.0', code: 'MINIMUM_RELEASE_AGE_VIOLATION', reason: 'published too recently' }
  let pass = 0
  installGlobalPackages.mockImplementation(async (...args: unknown[]) => {
    const installOpts = args[0] as { handleResolutionPolicyViolations?: (violations: unknown[]) => Promise<void> }
    const violations = pass++ === 0 ? [first] : [first, later]
    await installOpts.handleResolutionPolicyViolations?.(violations)
    return { ignoredBuilds: undefined, resolutionPolicyViolations: violations, resolvedVersions: { foo: '2.0.0' } }
  })
  const handleResolutionPolicyViolations = jest.fn<(violations: unknown[]) => Promise<void>>().mockResolvedValue(undefined)

  await handleGlobalUpdate({
    bin: '/global/bin',
    globalPkgDir: '/global/v11',
    handleResolutionPolicyViolations,
  } as any, [], {}) // eslint-disable-line @typescript-eslint/no-explicit-any

  expect(handleResolutionPolicyViolations).toHaveBeenCalledTimes(2)
  expect(handleResolutionPolicyViolations).toHaveBeenNthCalledWith(1, [first])
  expect(handleResolutionPolicyViolations).toHaveBeenNthCalledWith(2, [later])
})

test('global update aborts without installing when the immature version is not approved', async () => {
  createInstallDir.mockReturnValue('/global/v11/install-1')
  getHashLink.mockReturnValue('/global/v11/hash-foo')
  scanGlobalPackages.mockReturnValue([
    { dependencies: { foo: '^1.0.0' }, hash: 'hash-foo', installDir: '/global/v11/old-foo' },
  ])
  const violation = { name: 'foo', version: '2.0.0', code: 'MINIMUM_RELEASE_AGE_VIOLATION', reason: 'published too recently' }
  installGlobalPackages.mockImplementation(async (...args: unknown[]) => {
    const installOpts = args[0] as { handleResolutionPolicyViolations?: (violations: unknown[]) => Promise<void> }
    await installOpts.handleResolutionPolicyViolations?.([violation])
    return { ignoredBuilds: undefined, resolutionPolicyViolations: [violation], resolvedVersions: { foo: '2.0.0' } }
  })
  const handleResolutionPolicyViolations = jest.fn<(violations: unknown[]) => Promise<void>>()
    .mockRejectedValue(new Error('Aborted: the immature versions were not approved.'))

  await expect(handleGlobalUpdate({
    bin: '/global/bin',
    globalPkgDir: '/global/v11',
    handleResolutionPolicyViolations,
  } as any, [], {})) // eslint-disable-line @typescript-eslint/no-explicit-any
    .rejects.toThrow('Aborted: the immature versions were not approved.')

  expect(handleResolutionPolicyViolations).toHaveBeenCalledTimes(1)
  expect(installGlobalPackages).toHaveBeenCalledTimes(1)
  expect(installGlobalPackages).toHaveBeenCalledWith(
    expect.objectContaining({ lockfileOnly: true }),
    ['foo@^1.0.0']
  )
  expect(activateGlobalInstall).not.toHaveBeenCalled()
})
