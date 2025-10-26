import { type Stats } from 'fs'
import { checkDepsStatus, type CheckDepsStatusOptions } from '@pnpm/deps.status'
import * as workspaceStateModule from '@pnpm/workspace.state'
import * as lockfileFs from '@pnpm/lockfile.fs'
import { jest } from '@jest/globals'
import * as fsUtils from '../lib/safeStat.js'
import * as statManifestFileUtils from '../lib/statManifestFile.js'

jest.mock('../lib/safeStat', () => ({
  ...jest.requireActual<object>('../lib/safeStat'),
  safeStatSync: jest.fn(),
  safeStat: jest.fn(),
}))

jest.mock('../lib/statManifestFile', () => ({
  ...jest.requireActual<object>('../lib/statManifestFile'),
  statManifestFile: jest.fn(),
}))

jest.mock('@pnpm/lockfile.fs', () => ({
  ...jest.requireActual<object>('@pnpm/lockfile.fs'),
  readCurrentLockfile: jest.fn(),
  readWantedLockfile: jest.fn(),
}))

describe('checkDepsStatus - pnpmfile modification', () => {
  beforeEach(() => {
    jest.resetModules()
    jest.clearAllMocks()
  })

  it('returns upToDate: false when a pnpmfile was modified', async () => {
    const lastValidatedTimestamp = Date.now() - 10_000
    const beforeLastValidation = lastValidatedTimestamp - 10_000
    const afterLastValidation = lastValidatedTimestamp + 1_000
    const mockWorkspaceState: workspaceStateModule.WorkspaceState = {
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

    jest.spyOn(workspaceStateModule, 'loadWorkspaceState').mockReturnValue(mockWorkspaceState)

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
    const returnEmptyLockfile = async () => ({} as lockfileFs.LockfileObject)
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
