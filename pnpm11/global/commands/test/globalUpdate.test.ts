import { beforeEach, expect, jest, test } from '@jest/globals'

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
const activateGlobalInstall = jest.fn<(opts: unknown) => Promise<Set<string>>>().mockResolvedValue(new Set(['fresh']))
const cleanupReplacedGlobalInstalls = jest.fn<(opts: unknown) => Promise<void>>().mockResolvedValue(undefined)

jest.unstable_mockModule('@pnpm/core-loggers', () => ({ summaryLogger: { debug: summaryDebug } }))
jest.unstable_mockModule('@pnpm/global.packages', () => ({
  cleanOrphanedInstallDirs,
  createInstallDir,
  getHashLink,
  getInstalledBinNames,
  scanGlobalPackages,
}))
jest.unstable_mockModule('../src/checkGlobalBinConflicts.js', () => ({ checkGlobalBinConflicts }))
jest.unstable_mockModule('../src/globalActivation.js', () => ({ cleanupReplacedGlobalInstalls, activateGlobalInstall }))
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
  const groups = [
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
    groups: [groups[0]],
    globalDir: '/global/v11',
    globalBinDir: '/global/bin',
    activeHash: 'hash-foo',
    activatedBins: new Set(['fresh']),
    protectedBins: new Set(),
  })
  expect(cleanupReplacedGlobalInstalls).toHaveBeenNthCalledWith(2, {
    groups: [groups[1]],
    globalDir: '/global/v11',
    globalBinDir: '/global/bin',
    activeHash: 'hash-bar',
    activatedBins: new Set(['fresh']),
    protectedBins: new Set(),
  })
  for (const index of [0, 1]) {
    expect(activateGlobalInstall.mock.invocationCallOrder[index]).toBeLessThan(cleanupReplacedGlobalInstalls.mock.invocationCallOrder[index])
    expect(cleanupReplacedGlobalInstalls.mock.invocationCallOrder[index]).toBeLessThan(updateResolutionPolicyManifest.mock.invocationCallOrder[index])
  }
  expect(summaryDebug).toHaveBeenCalledTimes(1)
  expect(summaryDebug).toHaveBeenCalledWith({ prefix: '/global/v11' })
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
