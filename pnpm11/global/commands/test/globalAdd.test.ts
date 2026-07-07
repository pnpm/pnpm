import { expect, jest, test } from '@jest/globals'
import type { GlobalPackageInfo } from '@pnpm/global.packages'
import type { DependencyManifest } from '@pnpm/types'

type CheckGlobalBinConflictsOptions = {
  globalDir: string
  globalBinDir: string
  newPkgs: Array<{ manifest: DependencyManifest, location: string }>
  shouldSkip: (pkg: GlobalPackageInfo) => boolean
}

const linkBinsOfPackages = jest.fn<(pkgs: unknown[], globalBinDir: string, opts: { excludeBins: Set<string> }) => Promise<void>>().mockResolvedValue(undefined)
const removeBin = jest.fn<(cmd: string) => Promise<void>>().mockResolvedValue(undefined)
const cleanOrphanedInstallDirs = jest.fn()
const createGlobalCacheKey = jest.fn().mockReturnValue('new-hash')
const createInstallDir = jest.fn().mockReturnValue('/global/v11/new')
const findGlobalPackage = jest.fn<(globalDir: string, alias: string) => GlobalPackageInfo | null>()
const getHashLink = jest.fn((globalDir: string, hash: string) => `${globalDir}/${hash}`)
const getInstalledBinNames = jest.fn<() => Promise<string[]>>().mockResolvedValue(['pnpm'])
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
const symlinkDir = jest.fn<(src: string, dest: string, opts: { overwrite: boolean }) => Promise<void>>().mockResolvedValue(undefined)

jest.unstable_mockModule('@pnpm/bins.linker', () => ({ linkBinsOfPackages }))
jest.unstable_mockModule('@pnpm/bins.remover', () => ({ removeBin }))
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
jest.unstable_mockModule('is-subdir', () => ({ isSubdir: () => true }))
jest.unstable_mockModule('symlink-dir', () => ({ symlinkDir }))
jest.unstable_mockModule('../src/checkGlobalBinConflicts.js', () => ({ checkGlobalBinConflicts }))
jest.unstable_mockModule('../src/installGlobalPackages.js', () => ({ installGlobalPackages }))
jest.unstable_mockModule('../src/promptApproveGlobalBuilds.js', () => ({ promptApproveGlobalBuilds }))
jest.unstable_mockModule('../src/readInstalledPackages.js', () => ({ readInstalledPackages }))

const { getReplacementAliases, handleGlobalAdd, shouldReplaceExistingGlobalInstall } = await import('../src/globalAdd.js')

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

test('global add replaces an existing pnpm install when installing @pnpm/exe', async () => {
  const existingPnpm = {
    dependencies: { pnpm: '12.0.0-alpha.2' },
    hash: 'old-pnpm',
    installDir: '/global/v11/old-pnpm',
  }
  findGlobalPackage.mockImplementation((_globalDir: string, alias: string) => {
    return alias === 'pnpm' ? existingPnpm : null
  })
  checkGlobalBinConflicts.mockImplementation(async (opts) => {
    expect(opts.shouldSkip(existingPnpm)).toBe(true)
    return new Set()
  })

  await handleGlobalAdd({
    bin: '/global/bin',
    dir: '/project',
    globalPkgDir: '/global/v11',
    registries: {},
  } as any, ['file:/tmp/pnpm'], {}) // eslint-disable-line @typescript-eslint/no-explicit-any

  expect(findGlobalPackage).toHaveBeenCalledWith('/global/v11', '@pnpm/exe')
  expect(findGlobalPackage).toHaveBeenCalledWith('/global/v11', 'pnpm')
  expect(removeBin).toHaveBeenCalledWith('/global/bin/pnpm')
  expect(symlinkDir).toHaveBeenCalledWith('/global/v11/new', '/global/v11/new-hash', { overwrite: true })
  expect(linkBinsOfPackages).toHaveBeenCalledWith([], '/global/bin', { excludeBins: new Set() })
})
