import { checkDepsStatus, type CheckDepsStatusOptions } from '@pnpm/deps.status'
import * as workspaceStateModule from '@pnpm/workspace.state'
import * as lockfileFs from '@pnpm/lockfile.fs'
import * as fsUtils from '../lib/safeStat'
import * as statManifestFileUtils from '../lib/statManifestFile'

jest.mock('../lib/safeStat', () => ({
  ...jest.requireActual('../lib/safeStat'),
  safeStatSync: jest.fn(),
  safeStat: jest.fn(),
}))

jest.mock('../lib/statManifestFile', () => ({
  ...jest.requireActual('../lib/statManifestFile'),
  statManifestFile: jest.fn(),
}))

jest.mock('@pnpm/lockfile.fs', () => ({
  ...jest.requireActual('@pnpm/lockfile.fs'),
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

    ;(fsUtils.safeStatSync as jest.Mock).mockImplementation((filePath: string) => {
      if (filePath === 'pnpmfile.js') {
        return {
          mtime: new Date(beforeLastValidation),
          mtimeMs: beforeLastValidation,
        }
      }
      if (filePath === 'modifiedPnpmfile.js') {
        return {
          mtime: new Date(afterLastValidation),
          mtimeMs: afterLastValidation,
        }
      }
      return undefined
    })
    ;(fsUtils.safeStat as jest.Mock).mockImplementation(async () => {
      return {
        mtime: new Date(beforeLastValidation),
        mtimeMs: beforeLastValidation,
      }
    })
    ;(statManifestFileUtils.statManifestFile as jest.Mock).mockImplementation(async () => {
      return undefined
    })
    const returnEmptyLockfile = async () => ({})
    ;(lockfileFs.readCurrentLockfile as jest.Mock).mockImplementation(returnEmptyLockfile)
    ;(lockfileFs.readWantedLockfile as jest.Mock).mockImplementation(returnEmptyLockfile)

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
