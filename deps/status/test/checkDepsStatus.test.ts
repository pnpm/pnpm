import type { Stats } from 'fs'
import type { CheckDepsStatusOptions } from '@pnpm/deps.status'
import type { WorkspaceState } from '@pnpm/workspace.state'
import { jest } from '@jest/globals'
import type { LockfileObject } from '@pnpm/lockfile.fs'

{
  const original = await import('@pnpm/workspace.state')
  jest.unstable_mockModule('@pnpm/workspace.state', () => ({
    ...original,
    loadWorkspaceState: jest.fn(),
  }))
}
{
  const original = await import('../lib/safeStat.js')
  jest.unstable_mockModule('../lib/safeStat', () => ({
    ...original,
    safeStatSync: jest.fn(),
    safeStat: jest.fn(),
  }))
}
{
  const original = await import('../lib/statManifestFile.js')
  jest.unstable_mockModule('../lib/statManifestFile', () => ({
    ...original,
    statManifestFile: jest.fn(),
  }))
}
{
  const original = await import('@pnpm/lockfile.fs')
  jest.unstable_mockModule('@pnpm/lockfile.fs', () => ({
    ...original,
    readCurrentLockfile: jest.fn(),
    readWantedLockfile: jest.fn(),
  }))
}

const { checkDepsStatus } = await import('@pnpm/deps.status')
const { loadWorkspaceState } = await import('@pnpm/workspace.state')
const lockfileFs = await import('@pnpm/lockfile.fs')
const fsUtils = await import('../lib/safeStat.js')
const statManifestFileUtils = await import('../lib/statManifestFile.js')

describe('checkDepsStatus - settings change detection', () => {
  beforeEach(() => {
    jest.resetModules()
    jest.clearAllMocks()
  })

  it('returns upToDate: false when overrides have changed', async () => {
    const lastValidatedTimestamp = Date.now() - 10_000
    const mockWorkspaceState: WorkspaceState = {
      lastValidatedTimestamp,
      pnpmfiles: [],
      settings: {
        excludeLinksFromLockfile: false,
        linkWorkspacePackages: true,
        preferWorkspacePackages: true,
        overrides: { foo: '1.0.0' },
      },
      projects: {},
      filteredInstall: false,
    }

    jest.mocked(loadWorkspaceState).mockReturnValue(mockWorkspaceState)

    const opts: CheckDepsStatusOptions = {
      rootProjectManifest: {},
      rootProjectManifestDir: '/project',
      pnpmfile: [],
      ...mockWorkspaceState.settings,
      overrides: { foo: '2.0.0' },
    }
    const result = await checkDepsStatus(opts)

    expect(result.upToDate).toBe(false)
    expect(result.issue).toBe('The value of the overrides setting has changed')
  })

  it('returns upToDate: false when packageExtensions have changed', async () => {
    const lastValidatedTimestamp = Date.now() - 10_000
    const mockWorkspaceState: WorkspaceState = {
      lastValidatedTimestamp,
      pnpmfiles: [],
      settings: {
        excludeLinksFromLockfile: false,
        linkWorkspacePackages: true,
        preferWorkspacePackages: true,
        packageExtensions: { foo: { dependencies: { bar: '1.0.0' } } },
      },
      projects: {},
      filteredInstall: false,
    }

    jest.mocked(loadWorkspaceState).mockReturnValue(mockWorkspaceState)

    const opts: CheckDepsStatusOptions = {
      rootProjectManifest: {},
      rootProjectManifestDir: '/project',
      pnpmfile: [],
      ...mockWorkspaceState.settings,
      packageExtensions: { foo: { dependencies: { bar: '2.0.0' } } },
    }
    const result = await checkDepsStatus(opts)

    expect(result.upToDate).toBe(false)
    expect(result.issue).toBe('The value of the packageExtensions setting has changed')
  })

  it('returns upToDate: false when ignoredOptionalDependencies have changed', async () => {
    const lastValidatedTimestamp = Date.now() - 10_000
    const mockWorkspaceState: WorkspaceState = {
      lastValidatedTimestamp,
      pnpmfiles: [],
      settings: {
        excludeLinksFromLockfile: false,
        linkWorkspacePackages: true,
        preferWorkspacePackages: true,
        ignoredOptionalDependencies: ['foo'],
      },
      projects: {},
      filteredInstall: false,
    }

    jest.mocked(loadWorkspaceState).mockReturnValue(mockWorkspaceState)

    const opts: CheckDepsStatusOptions = {
      rootProjectManifest: {},
      rootProjectManifestDir: '/project',
      pnpmfile: [],
      ...mockWorkspaceState.settings,
      ignoredOptionalDependencies: ['foo', 'bar'],
    }
    const result = await checkDepsStatus(opts)

    expect(result.upToDate).toBe(false)
    expect(result.issue).toBe('The value of the ignoredOptionalDependencies setting has changed')
  })

  it('returns upToDate: false when patchedDependencies have changed', async () => {
    const lastValidatedTimestamp = Date.now() - 10_000
    const mockWorkspaceState: WorkspaceState = {
      lastValidatedTimestamp,
      pnpmfiles: [],
      settings: {
        excludeLinksFromLockfile: false,
        linkWorkspacePackages: true,
        preferWorkspacePackages: true,
        patchedDependencies: { foo: 'patches/foo.patch' },
      },
      projects: {},
      filteredInstall: false,
    }

    jest.mocked(loadWorkspaceState).mockReturnValue(mockWorkspaceState)

    const opts: CheckDepsStatusOptions = {
      rootProjectManifest: {},
      rootProjectManifestDir: '/project',
      pnpmfile: [],
      ...mockWorkspaceState.settings,
      patchedDependencies: { foo: 'patches/foo-v2.patch' },
    }
    const result = await checkDepsStatus(opts)

    expect(result.upToDate).toBe(false)
    expect(result.issue).toBe('The value of the patchedDependencies setting has changed')
  })

  it('returns upToDate: false when peersSuffixMaxLength has changed', async () => {
    const lastValidatedTimestamp = Date.now() - 10_000
    const mockWorkspaceState: WorkspaceState = {
      lastValidatedTimestamp,
      pnpmfiles: [],
      settings: {
        excludeLinksFromLockfile: false,
        linkWorkspacePackages: true,
        preferWorkspacePackages: true,
        peersSuffixMaxLength: 1000,
      },
      projects: {},
      filteredInstall: false,
    }

    jest.mocked(loadWorkspaceState).mockReturnValue(mockWorkspaceState)

    const opts: CheckDepsStatusOptions = {
      rootProjectManifest: {},
      rootProjectManifestDir: '/project',
      pnpmfile: [],
      ...mockWorkspaceState.settings,
      peersSuffixMaxLength: 100,
    }
    const result = await checkDepsStatus(opts)

    expect(result.upToDate).toBe(false)
    expect(result.issue).toBe('The value of the peersSuffixMaxLength setting has changed')
  })
})

describe('checkDepsStatus - pnpmfile modification', () => {
  beforeEach(() => {
    jest.resetModules()
    jest.clearAllMocks()
  })

  it('returns upToDate: false when a pnpmfile was modified', async () => {
    const lastValidatedTimestamp = Date.now() - 10_000
    const beforeLastValidation = lastValidatedTimestamp - 10_000
    const afterLastValidation = lastValidatedTimestamp + 1_000
    const mockWorkspaceState: WorkspaceState = {
      lastValidatedTimestamp,
      pnpmfiles: ['pnpmfile.js', 'modifiedPnpmfile.js'],
      settings: {
        excludeLinksFromLockfile: false,
        linkWorkspacePackages: true,
        preferWorkspacePackages: true,
      },
      projects: {},
      filteredInstall: false,
    }

    jest.mocked(loadWorkspaceState).mockReturnValue(mockWorkspaceState)

    jest.mocked(fsUtils.safeStatSync).mockImplementation((filePath: string) => {
      if (filePath === 'pnpmfile.js') {
        return {
          mtime: new Date(beforeLastValidation),
          mtimeMs: beforeLastValidation,
        } as Stats
      }
      if (filePath === 'modifiedPnpmfile.js') {
        return {
          mtime: new Date(afterLastValidation),
          mtimeMs: afterLastValidation,
        } as Stats
      }
      return undefined
    })
    jest.mocked(fsUtils.safeStat).mockImplementation(async () => {
      return {
        mtime: new Date(beforeLastValidation),
        mtimeMs: beforeLastValidation,
      } as Stats
    })
    jest.mocked(statManifestFileUtils.statManifestFile).mockImplementation(async () => {
      return undefined
    })
    const returnEmptyLockfile = async () => ({} as LockfileObject)
    jest.mocked(lockfileFs.readCurrentLockfile).mockImplementation(returnEmptyLockfile)
    jest.mocked(lockfileFs.readWantedLockfile).mockImplementation(returnEmptyLockfile)

    const opts: CheckDepsStatusOptions = {
      rootProjectManifest: {},
      rootProjectManifestDir: '/project',
      pnpmfile: mockWorkspaceState.pnpmfiles,
      ...mockWorkspaceState.settings,
    }
    const result = await checkDepsStatus(opts)

    expect(result.upToDate).toBe(false)
    expect(result.issue).toBe('pnpmfile at "modifiedPnpmfile.js" was modified')
  })
})
