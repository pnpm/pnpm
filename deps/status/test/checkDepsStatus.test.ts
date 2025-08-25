import { type Stats } from 'fs'
import { type CheckDepsStatusOptions } from '@pnpm/deps.status'
import { type WorkspaceState } from '@pnpm/workspace.state'
import { jest } from '@jest/globals'
import { type LockfileObject } from '@pnpm/lockfile.fs'

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
