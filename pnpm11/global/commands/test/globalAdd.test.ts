import fs from 'node:fs'
import os from 'node:os'
import path from 'node:path'

import { beforeEach, expect, jest, test } from '@jest/globals'
import type { GlobalPackageInfo } from '@pnpm/global.packages'
import type { DependencyManifest } from '@pnpm/types'

type CheckGlobalBinConflictsOptions = {
  globalDir: string
  globalBinDir: string
  newPkgs: Array<{ manifest: DependencyManifest, location: string }>
  shouldSkip: (pkg: GlobalPackageInfo) => boolean
}

const cleanOrphanedInstallDirs = jest.fn()
const createGlobalCacheKey = jest.fn().mockReturnValue('new-hash')
const createInstallDir = jest.fn().mockReturnValue('/global/v11/new')
const findGlobalPackage = jest.fn<(globalDir: string, alias: string) => GlobalPackageInfo | null>()
const getHashLink = jest.fn((globalDir: string, hash: string) => `${globalDir}/${hash}`)
const getInstalledBinNames = jest.fn<(pkg: GlobalPackageInfo) => Promise<string[]>>().mockResolvedValue(['pnpm'])
const scanGlobalPackages = jest.fn().mockReturnValue([])
const readPackageJsonFromDirRawSync = jest.fn().mockReturnValue({
  dependencies: { '@pnpm/exe': 'file:/tmp/pnpm' },
})
const checkGlobalBinConflicts = jest.fn<(opts: CheckGlobalBinConflictsOptions) => Promise<Set<string>>>().mockResolvedValue(new Set())
const installGlobalPackages = jest.fn<(...args: unknown[]) => Promise<{ ignoredBuilds: undefined, resolutionPolicyViolations: [] }>>()
  .mockResolvedValue({ ignoredBuilds: undefined, resolutionPolicyViolations: [] })
const promptApproveGlobalBuilds = jest.fn<() => Promise<void>>().mockResolvedValue(undefined)
const readInstalledPackages = jest.fn<() => Promise<[]>>().mockResolvedValue([])
const summaryDebug = jest.fn()
const activateGlobalInstall = jest.fn<(opts: unknown) => Promise<Set<string>>>().mockResolvedValue(new Set(['pnpm']))
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
  createGlobalCacheKey,
  createInstallDir,
  findGlobalPackage,
  getHashLink,
  getInstalledBinNames,
  scanGlobalPackages,
}))
jest.unstable_mockModule('@pnpm/pkg-manifest.reader', () => ({
  readPackageJsonFromDirRawSync,
}))
jest.unstable_mockModule('../src/checkGlobalBinConflicts.js', () => ({ checkGlobalBinConflicts }))
jest.unstable_mockModule('../src/installGlobalPackages.js', () => ({ installGlobalPackages }))
jest.unstable_mockModule('../src/globalActivation.js', () => ({
  activateGlobalInstall,
  cleanupFailedGlobalInstall,
  cleanupReplacedGlobalInstalls,
}))
jest.unstable_mockModule('../src/promptApproveGlobalBuilds.js', () => ({ promptApproveGlobalBuilds }))
jest.unstable_mockModule('../src/readInstalledPackages.js', () => ({ readInstalledPackages }))

const { getReplacementAliases, handleGlobalAdd, shouldReplaceExistingGlobalInstall } = await import('../src/globalAdd.js')

beforeEach(() => {
  jest.clearAllMocks()
  checkGlobalBinConflicts.mockResolvedValue(new Set())
  cleanupReplacedGlobalInstalls.mockResolvedValue(undefined)
  createInstallDir.mockReturnValue('/global/v11/new')
  createGlobalCacheKey.mockReturnValue('new-hash')
  findGlobalPackage.mockReturnValue(null)
  getInstalledBinNames.mockResolvedValue(['pnpm'])
  activateGlobalInstall.mockResolvedValue(new Set(['pnpm']))
  scanGlobalPackages.mockReturnValue([])
})

test('global add treats pnpm and @pnpm/exe as replacement aliases', () => {
  expect(getReplacementAliases(['@pnpm/exe'])).toStrictEqual(['@pnpm/exe', 'pnpm'])
  expect(getReplacementAliases(['pnpm'])).toStrictEqual(['pnpm', '@pnpm/exe'])
})

test('global add does not expand unrelated replacement aliases', () => {
  expect(getReplacementAliases(['eslint', 'typescript'])).toStrictEqual(['eslint', 'typescript'])
})

test('global add only uses pnpm alias equivalence for pnpm-only existing groups', () => {
  const aliases = ['@pnpm/exe']
  const replacementAliases = getReplacementAliases(aliases)

  expect(shouldReplaceExistingGlobalInstall({
    dependencies: { pnpm: '12.0.0-alpha.2' },
    hash: 'old-pnpm',
    installDir: '/global/v11/old-pnpm',
  }, aliases, replacementAliases)).toBe(true)
  expect(shouldReplaceExistingGlobalInstall({
    dependencies: {
      pnpm: '12.0.0-alpha.2',
      eslint: '^9.0.0',
    },
    hash: 'mixed-group',
    installDir: '/global/v11/mixed-group',
  }, aliases, replacementAliases)).toBe(false)
})

test('global add still replaces exact aliases in mixed existing groups', () => {
  const aliases = ['@pnpm/exe']
  const replacementAliases = getReplacementAliases(aliases)

  expect(shouldReplaceExistingGlobalInstall({
    dependencies: {
      '@pnpm/exe': 'file:/tmp/pnpm',
      eslint: '^9.0.0',
    },
    hash: 'mixed-exact-group',
    installDir: '/global/v11/mixed-exact-group',
  }, aliases, replacementAliases)).toBe(true)
})

test('global add activates before cleaning up a same-hash pnpm replacement', async () => {
  const existingPnpm = {
    dependencies: { pnpm: '12.0.0-alpha.2' },
    hash: 'old-pnpm',
    installDir: '/global/v11/old-pnpm',
  }
  const survivor = {
    dependencies: { eslint: '^9.0.0' },
    hash: 'eslint',
    installDir: '/global/v11/eslint',
  }
  const activatedBins = new Set(['pnpm'])
  const updateResolutionPolicyManifest = jest.fn<() => Promise<void>>().mockResolvedValue(undefined)
  createGlobalCacheKey.mockReturnValue('old-pnpm')
  findGlobalPackage.mockImplementation((_globalDir: string, alias: string) => {
    return alias === 'pnpm' ? existingPnpm : null
  })
  scanGlobalPackages.mockReturnValue([existingPnpm, survivor])
  getInstalledBinNames.mockImplementation(async (pkg) => pkg === existingPnpm ? ['pnpm'] : ['eslint'])
  activateGlobalInstall.mockResolvedValue(activatedBins)
  checkGlobalBinConflicts.mockImplementation(async (opts) => {
    expect(opts.shouldSkip(existingPnpm)).toBe(true)
    return new Set()
  })

  await handleGlobalAdd({
    bin: '/global/bin',
    dir: '/project',
    globalPkgDir: '/global/v11',
    registriesByScope: {},
    updateResolutionPolicyManifest,
  } as any, ['file:/tmp/pnpm'], {}) // eslint-disable-line @typescript-eslint/no-explicit-any

  expect(findGlobalPackage).toHaveBeenCalledWith('/global/v11', '@pnpm/exe')
  expect(findGlobalPackage).toHaveBeenCalledWith('/global/v11', 'pnpm')
  expect(activateGlobalInstall).toHaveBeenCalledWith({
    installDir: '/global/v11/new',
    hashLink: '/global/v11/old-pnpm',
    globalBinDir: '/global/bin',
    pkgs: [],
    binsToSkip: new Set(),
  })
  expect(cleanupReplacedGlobalInstalls).toHaveBeenCalledWith({
    groups: [{ info: existingPnpm, binNames: ['pnpm'] }],
    globalDir: '/global/v11',
    globalBinDir: '/global/bin',
    activeHash: 'old-pnpm',
    activatedBins,
    protectedBins: new Set(['eslint']),
  })
  expect(getInstalledBinNames).toHaveBeenCalledTimes(2)
  expect(getInstalledBinNames).toHaveBeenCalledWith(existingPnpm)
  expect(getInstalledBinNames).toHaveBeenCalledWith(survivor)
  for (const callOrder of getInstalledBinNames.mock.invocationCallOrder) {
    expect(callOrder).toBeLessThan(activateGlobalInstall.mock.invocationCallOrder[0])
  }
  expect(activateGlobalInstall.mock.invocationCallOrder[0]).toBeLessThan(cleanupReplacedGlobalInstalls.mock.invocationCallOrder[0])
  expect(cleanupReplacedGlobalInstalls.mock.invocationCallOrder[0]).toBeLessThan(updateResolutionPolicyManifest.mock.invocationCallOrder[0])
})

test('global add retries safely and activates from a complete replacement ownership snapshot after repair', async () => {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'global-add-ownership-'))
  expect(fs.realpathSync(path.dirname(root))).toBe(fs.realpathSync(os.tmpdir()))
  const globalDir = path.join(root, 'global')
  const globalBinDir = path.join(root, 'bin')
  const oldInstallDir = path.join(globalDir, 'old-install')
  const survivorInstallDir = path.join(globalDir, 'eslint-install')
  const oldHashLink = path.join(globalDir, 'old-pnpm')
  const oldMarker = path.join(oldInstallDir, 'marker')
  fs.mkdirSync(oldInstallDir, { recursive: true })
  fs.mkdirSync(survivorInstallDir, { recursive: true })
  fs.mkdirSync(globalBinDir, { recursive: true })
  fs.writeFileSync(oldMarker, 'old install\n')
  fs.writeFileSync(path.join(survivorInstallDir, 'marker'), 'survivor install\n')
  fs.symlinkSync(oldInstallDir, oldHashLink, process.platform === 'win32' ? 'junction' : 'dir')

  const existingPnpm = {
    dependencies: { pnpm: '12.0.0-alpha.2' },
    hash: 'old-pnpm',
    installDir: oldInstallDir,
  }
  const survivor = {
    dependencies: { eslint: '^9.0.0' },
    hash: 'eslint',
    installDir: survivorInstallDir,
  }
  const enumerationError = Object.assign(new Error('replacement package.json is missing'), { code: 'ENOENT' })
  const freshInstallDirs: string[] = []
  createInstallDir.mockImplementation(() => {
    const freshInstallDir = path.join(globalDir, `fresh-install-${freshInstallDirs.length + 1}`)
    const relative = path.relative(root, freshInstallDir)
    expect(path.isAbsolute(relative) || relative === '..' || relative.startsWith(`..${path.sep}`)).toBe(false)
    fs.mkdirSync(freshInstallDir, { recursive: true })
    fs.writeFileSync(path.join(freshInstallDir, 'marker'), 'fresh install\n')
    freshInstallDirs.push(freshInstallDir)
    return freshInstallDir
  })
  createGlobalCacheKey.mockReturnValue('old-pnpm')
  findGlobalPackage.mockImplementation((_globalDir: string, alias: string) => {
    return alias === 'pnpm' ? existingPnpm : null
  })
  scanGlobalPackages.mockReturnValue([existingPnpm, survivor])
  let ownershipBroken = true
  getInstalledBinNames.mockImplementation(async (pkg) => {
    if (pkg === existingPnpm && ownershipBroken) throw enumerationError
    if (pkg === existingPnpm) return ['pnpm']
    return ['eslint']
  })
  const add = async (): Promise<void> => handleGlobalAdd({
    bin: globalBinDir,
    dir: root,
    globalPkgDir: globalDir,
    registriesByScope: {},
  } as any, ['file:/tmp/pnpm'], {}) // eslint-disable-line @typescript-eslint/no-explicit-any
  const snapshotBeforeActivation = (): unknown => ({
    binEntries: fs.readdirSync(globalBinDir).sort(),
    globalEntries: fs.readdirSync(globalDir).sort(),
    oldHashTarget: fs.realpathSync(oldHashLink),
    oldMarker: fs.readFileSync(oldMarker, 'utf8'),
    survivorMarker: fs.readFileSync(path.join(survivorInstallDir, 'marker'), 'utf8'),
  })
  const before = snapshotBeforeActivation()
  const assertFailedAttempt = async (attempt: number): Promise<void> => {
    let failure: unknown
    try {
      await add()
    } catch (err) {
      failure = err
    }
    expect({ attempt, failure }).toStrictEqual({ attempt, failure: enumerationError })
    expect(snapshotBeforeActivation()).toStrictEqual(before)
    expect(fs.existsSync(freshInstallDirs[attempt - 1])).toBe(false)
    expect(activateGlobalInstall).not.toHaveBeenCalled()
    expect(cleanupReplacedGlobalInstalls).not.toHaveBeenCalled()
  }

  try {
    await assertFailedAttempt(1)
    await assertFailedAttempt(2)

    ownershipBroken = false
    let ownershipReadsAtSwitch = 0
    activateGlobalInstall.mockImplementation(async (opts) => {
      ownershipReadsAtSwitch = getInstalledBinNames.mock.calls.length
      const installDir = (opts as { installDir: string }).installDir
      fs.rmSync(oldHashLink, { force: true })
      fs.symlinkSync(installDir, oldHashLink, process.platform === 'win32' ? 'junction' : 'dir')
      return new Set(['pnpm'])
    })

    await add()

    expect(activateGlobalInstall).toHaveBeenCalledTimes(1)
    expect(cleanupReplacedGlobalInstalls).toHaveBeenCalledTimes(1)
    expect(cleanupReplacedGlobalInstalls).toHaveBeenCalledWith({
      groups: [{ info: existingPnpm, binNames: ['pnpm'] }],
      globalBinDir,
      globalDir,
      activeHash: 'old-pnpm',
      activatedBins: new Set(['pnpm']),
      protectedBins: new Set(['eslint']),
    })
    expect(getInstalledBinNames).toHaveBeenCalledTimes(ownershipReadsAtSwitch)
    expect(freshInstallDirs).toHaveLength(3)
    expect(fs.realpathSync(oldHashLink)).toBe(fs.realpathSync(freshInstallDirs[2]))
    expect(fs.readFileSync(oldMarker, 'utf8')).toBe('old install\n')
  } finally {
    fs.rmSync(root, { recursive: true, force: true })
  }
})

test('global add preserves ownership state and both errors when fresh install cleanup fails', async () => {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'global-add-cleanup-failure-'))
  expect(fs.realpathSync(path.dirname(root))).toBe(fs.realpathSync(os.tmpdir()))
  const globalDir = path.join(root, 'global')
  const globalBinDir = path.join(root, 'bin')
  const oldInstallDir = path.join(globalDir, 'old-install')
  const survivorInstallDir = path.join(globalDir, 'survivor-install')
  const freshInstallDir = path.join(globalDir, 'fresh-install')
  const oldHashLink = path.join(globalDir, 'old-pnpm')
  const oldMarker = path.join(oldInstallDir, 'marker')
  const oldBin = path.join(globalBinDir, 'pnpm')
  fs.mkdirSync(oldInstallDir, { recursive: true })
  fs.mkdirSync(survivorInstallDir, { recursive: true })
  fs.mkdirSync(freshInstallDir, { recursive: true })
  fs.mkdirSync(globalBinDir, { recursive: true })
  fs.writeFileSync(oldMarker, 'old install\n')
  fs.writeFileSync(path.join(survivorInstallDir, 'marker'), 'survivor install\n')
  fs.writeFileSync(path.join(freshInstallDir, 'marker'), 'fresh install\n')
  fs.writeFileSync(oldBin, 'old pnpm shim\n')
  fs.symlinkSync(oldInstallDir, oldHashLink, process.platform === 'win32' ? 'junction' : 'dir')

  const existingPnpm = {
    dependencies: { pnpm: '12.0.0-alpha.2' },
    hash: 'old-pnpm',
    installDir: oldInstallDir,
  }
  const survivor = {
    dependencies: { eslint: '^9.0.0' },
    hash: 'eslint',
    installDir: survivorInstallDir,
  }
  const enumerationError = Object.assign(new Error('replacement package.json is missing'), { code: 'ENOENT' })
  const cleanupError = Object.assign(new Error('fresh install cleanup failed'), { code: 'EACCES' })
  createInstallDir.mockReturnValue(freshInstallDir)
  createGlobalCacheKey.mockReturnValue(existingPnpm.hash)
  findGlobalPackage.mockImplementation((_globalDir: string, alias: string) => alias === 'pnpm' ? existingPnpm : null)
  scanGlobalPackages.mockReturnValue([existingPnpm, survivor])
  getInstalledBinNames.mockImplementation(async (pkg) => {
    if (pkg === existingPnpm) throw enumerationError
    return ['eslint']
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
  const rmSpy = jest.spyOn(fs.promises, 'rm').mockImplementation(async (target, options) => {
    if (path.resolve(String(target)) === path.resolve(freshInstallDir)) throw cleanupError
    await realRm(target, options)
  })

  try {
    let thrown: unknown
    try {
      await handleGlobalAdd({
        bin: globalBinDir,
        dir: root,
        globalPkgDir: globalDir,
        registriesByScope: {},
      } as any, ['file:/tmp/pnpm'], {}) // eslint-disable-line @typescript-eslint/no-explicit-any
    } catch (err) {
      thrown = err
    }

    expect(thrown).toBeInstanceOf(AggregateError)
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

test('global add does not clean up or persist policy when activation fails', async () => {
  const activationError = new Error('activation failed')
  const updateResolutionPolicyManifest = jest.fn<() => Promise<void>>().mockResolvedValue(undefined)
  activateGlobalInstall.mockRejectedValue(activationError)

  await expect(handleGlobalAdd({
    bin: '/global/bin',
    dir: '/project',
    globalPkgDir: '/global/v11',
    registriesByScope: {},
    updateResolutionPolicyManifest,
  } as any, ['file:/tmp/pnpm'], {})).rejects.toBe(activationError) // eslint-disable-line @typescript-eslint/no-explicit-any

  expect(cleanupReplacedGlobalInstalls).not.toHaveBeenCalled()
  expect(updateResolutionPolicyManifest).not.toHaveBeenCalled()
})
