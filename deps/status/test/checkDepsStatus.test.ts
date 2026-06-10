import type { Stats } from 'node:fs'
import fs from 'node:fs/promises'
import os from 'node:os'
import path from 'node:path'

import { beforeEach, describe, expect, it, jest } from '@jest/globals'
import type { CheckDepsStatusOptions } from '@pnpm/deps.status'
import type { LockfileObject } from '@pnpm/lockfile.fs'
import type { ProjectRootDir, ProjectRootDirRealPath } from '@pnpm/types'
import type { WorkspaceState } from '@pnpm/workspace.state'

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

  it('returns upToDate: false when allowBuilds have changed', async () => {
    const lastValidatedTimestamp = Date.now() - 10_000
    const mockWorkspaceState: WorkspaceState = {
      lastValidatedTimestamp,
      pnpmfiles: [],
      settings: {
        excludeLinksFromLockfile: false,
        linkWorkspacePackages: true,
        preferWorkspacePackages: true,
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
      allowBuilds: { sqlite3: false },
    }
    const result = await checkDepsStatus(opts)

    expect(result.upToDate).toBe(false)
    expect(result.issue).toBe('The value of the allowBuilds setting has changed')
  })

  it('skips the allowBuilds change detection when allowBuilds is in ignoredWorkspaceStateSettings', async () => {
    const lastValidatedTimestamp = Date.now() - 10_000
    const mockWorkspaceState: WorkspaceState = {
      lastValidatedTimestamp,
      pnpmfiles: [],
      settings: {
        excludeLinksFromLockfile: false,
        linkWorkspacePackages: true,
        preferWorkspacePackages: true,
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
      allowBuilds: { sqlite3: false },
      ignoredWorkspaceStateSettings: ['allowBuilds'],
    }
    const result = await checkDepsStatus(opts)

    expect(result.issue).not.toBe('The value of the allowBuilds setting has changed')
  })

  it('returns upToDate: false when enableGlobalVirtualStore is toggled off', async () => {
    const lastValidatedTimestamp = Date.now() - 10_000
    const mockWorkspaceState: WorkspaceState = {
      lastValidatedTimestamp,
      pnpmfiles: [],
      settings: {
        excludeLinksFromLockfile: false,
        linkWorkspacePackages: true,
        preferWorkspacePackages: true,
        enableGlobalVirtualStore: true,
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
      enableGlobalVirtualStore: false,
    }
    const result = await checkDepsStatus(opts)

    expect(result.upToDate).toBe(false)
    expect(result.issue).toBe('The value of the enableGlobalVirtualStore setting has changed')
  })

  it('returns upToDate: false when enableGlobalVirtualStore is toggled on from a legacy state file that lacks the key', async () => {
    const lastValidatedTimestamp = Date.now() - 10_000
    const mockWorkspaceState: WorkspaceState = {
      lastValidatedTimestamp,
      pnpmfiles: [],
      settings: {
        excludeLinksFromLockfile: false,
        linkWorkspacePackages: true,
        preferWorkspacePackages: true,
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
      enableGlobalVirtualStore: true,
    }
    const result = await checkDepsStatus(opts)

    expect(result.upToDate).toBe(false)
    expect(result.issue).toBe('The value of the enableGlobalVirtualStore setting has changed')
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

  it('returns upToDate: false when a patch was modified and manifests were not modified', async () => {
    const lastValidatedTimestamp = Date.now() - 10_000
    const beforeLastValidation = lastValidatedTimestamp - 10_000
    const afterLastValidation = lastValidatedTimestamp + 1_000
    const projectRootDir = '/project' as ProjectRootDir
    const projectRootDirRealPath = '/project' as ProjectRootDirRealPath
    const mockWorkspaceState: WorkspaceState = {
      lastValidatedTimestamp,
      pnpmfiles: [],
      settings: {
        excludeLinksFromLockfile: false,
        linkWorkspacePackages: true,
        preferWorkspacePackages: true,
        patchedDependencies: {
          foo: '/project/patches/foo.patch',
        },
      },
      projects: {
        [projectRootDir]: {
          name: 'root',
          version: '1.0.0',
        },
      },
      filteredInstall: false,
    }

    jest.mocked(loadWorkspaceState).mockReturnValue(mockWorkspaceState)

    jest.mocked(fsUtils.safeStat).mockImplementation(async (filePath: string) => {
      if (filePath === '/project/patches/foo.patch') {
        return {
          mtime: new Date(afterLastValidation),
          mtimeMs: afterLastValidation,
        } as Stats
      }
      return {
        mtime: new Date(beforeLastValidation),
        mtimeMs: beforeLastValidation,
      } as Stats
    })
    jest.mocked(statManifestFileUtils.statManifestFile).mockImplementation(async () => ({
      mtime: new Date(beforeLastValidation),
      mtimeMs: beforeLastValidation,
    } as Stats))

    const opts: CheckDepsStatusOptions = {
      allProjects: [{
        rootDir: projectRootDir,
        rootDirRealPath: projectRootDirRealPath,
        manifest: {
          name: 'root',
          version: '1.0.0',
          dependencies: {
            foo: '1.0.0',
          },
        },
        writeProjectManifest: async () => {},
      }],
      workspaceDir: '/project',
      rootProjectManifest: {},
      rootProjectManifestDir: '/project',
      pnpmfile: [],
      patchedDependencies: {
        foo: '/project/patches/foo.patch',
      },
      ...mockWorkspaceState.settings,
    }
    const result = await checkDepsStatus(opts)

    expect(result.upToDate).toBe(false)
    expect(result.issue).toBe('Patches were modified')
  })
})

describe('checkDepsStatus - lockfile conflicts', () => {
  beforeEach(() => {
    jest.resetModules()
    jest.clearAllMocks()
  })

  it('returns upToDate: false when the wanted lockfile has merge conflict markers', async () => {
    const projectDir = await fs.mkdtemp(path.join(os.tmpdir(), 'pnpm-check-deps-'))
    try {
      await writeConflictedLockfile(projectDir)
      const mockWorkspaceState: WorkspaceState = {
        lastValidatedTimestamp: Date.now() - 10_000,
        pnpmfiles: [],
        settings: {
          excludeLinksFromLockfile: false,
          linkWorkspacePackages: true,
          preferWorkspacePackages: true,
        },
        projects: {},
        filteredInstall: false,
      }

      jest.mocked(loadWorkspaceState).mockReturnValue(mockWorkspaceState)

      const opts: CheckDepsStatusOptions = {
        rootProjectManifest: {},
        rootProjectManifestDir: projectDir,
        pnpmfile: [],
        ...mockWorkspaceState.settings,
      }
      const result = await checkDepsStatus(opts)

      expect(result.upToDate).toBe(false)
      expect(result.issue).toBe(`The lockfile in ${projectDir} has merge conflicts`)
    } finally {
      await fs.rm(projectDir, { force: true, recursive: true })
    }
  })

  it('returns upToDate: false when a project lockfile has merge conflict markers and sharedWorkspaceLockfile is false', async () => {
    const workspaceDir = await fs.mkdtemp(path.join(os.tmpdir(), 'pnpm-check-deps-workspace-'))
    try {
      const projectDir = path.join(workspaceDir, 'packages/project')
      await fs.mkdir(projectDir, { recursive: true })
      await writeConflictedLockfile(projectDir)
      const projectRootDir = projectDir as ProjectRootDir
      const projectRootDirRealPath = await fs.realpath(projectDir) as ProjectRootDirRealPath
      const mockWorkspaceState: WorkspaceState = {
        lastValidatedTimestamp: Date.now() - 10_000,
        pnpmfiles: [],
        settings: {
          excludeLinksFromLockfile: false,
          linkWorkspacePackages: true,
          preferWorkspacePackages: true,
        },
        projects: {
          [projectRootDir]: {
            name: 'project',
            version: '1.0.0',
          },
        },
        filteredInstall: false,
      }

      jest.mocked(loadWorkspaceState).mockReturnValue(mockWorkspaceState)

      const opts: CheckDepsStatusOptions = {
        allProjects: [{
          rootDir: projectRootDir,
          rootDirRealPath: projectRootDirRealPath,
          manifest: {
            name: 'project',
            version: '1.0.0',
          },
          writeProjectManifest: async () => {},
        }],
        workspaceDir,
        rootProjectManifest: {},
        rootProjectManifestDir: workspaceDir,
        pnpmfile: [],
        sharedWorkspaceLockfile: false,
        ...mockWorkspaceState.settings,
      }
      const result = await checkDepsStatus(opts)

      expect(result.upToDate).toBe(false)
      expect(result.issue).toBe(`The lockfile in ${projectDir} has merge conflicts`)
    } finally {
      await fs.rm(workspaceDir, { force: true, recursive: true })
    }
  })
})

async function writeConflictedLockfile (lockfileDir: string): Promise<void> {
  await fs.writeFile(path.join(lockfileDir, 'pnpm-lock.yaml'), [
    "lockfileVersion: '9.0'",
    '<<<<<<< HEAD',
    'settings:',
    '  autoInstallPeers: true',
    '=======',
    'settings:',
    '  autoInstallPeers: false',
    '>>>>>>> branch',
    '',
  ].join('\n'))
}

describe('checkDepsStatus - missing wanted lockfile fallback', () => {
  beforeEach(() => {
    jest.resetModules()
    jest.clearAllMocks()
  })

  const currentLockfile = {
    lockfileVersion: '9.0',
    importers: { '.': { specifiers: {} } },
  } as unknown as LockfileObject

  function mockSingleProjectStats (opts: {
    wantedLockfileExists: boolean
    currentLockfileMtime: number
    manifestMtime: number
  }): void {
    jest.mocked(fsUtils.safeStat).mockImplementation(async (filePath: string) => {
      if (filePath === path.join('/project', 'node_modules', '.pnpm', 'lock.yaml')) {
        return {
          mtime: new Date(opts.currentLockfileMtime),
          mtimeMs: opts.currentLockfileMtime,
        } as Stats
      }
      if (filePath === path.join('/project', 'pnpm-lock.yaml') && opts.wantedLockfileExists) {
        return {
          mtime: new Date(opts.currentLockfileMtime),
          mtimeMs: opts.currentLockfileMtime,
        } as Stats
      }
      return undefined
    })
    jest.mocked(fsUtils.safeStatSync).mockReturnValue(undefined)
    jest.mocked(statManifestFileUtils.statManifestFile).mockImplementation(async () => ({
      mtime: new Date(opts.manifestMtime),
      mtimeMs: opts.manifestMtime,
    } as Stats))
    jest.mocked(lockfileFs.readCurrentLockfile).mockImplementation(async () => currentLockfile)
    jest.mocked(lockfileFs.readWantedLockfile).mockImplementation(async () => null)
  }

  it('returns the current lockfile to restore when pnpm-lock.yaml is missing in a single project', async () => {
    const lastValidatedTimestamp = Date.now() - 10_000
    const mockWorkspaceState: WorkspaceState = {
      lastValidatedTimestamp,
      pnpmfiles: [],
      settings: {
        excludeLinksFromLockfile: false,
        linkWorkspacePackages: true,
        preferWorkspacePackages: true,
      },
      projects: {},
      filteredInstall: false,
    }
    jest.mocked(loadWorkspaceState).mockReturnValue(mockWorkspaceState)
    mockSingleProjectStats({
      wantedLockfileExists: false,
      currentLockfileMtime: lastValidatedTimestamp - 10_000,
      manifestMtime: lastValidatedTimestamp - 20_000,
    })

    const opts: CheckDepsStatusOptions = {
      rootProjectManifest: {},
      rootProjectManifestDir: '/project',
      pnpmfile: [],
      ...mockWorkspaceState.settings,
    }
    const result = await checkDepsStatus(opts)

    expect(result.upToDate).toBe(true)
    expect(result.wantedLockfileToRestore).toEqual({
      lockfile: currentLockfile,
      lockfileDir: '/project',
    })
  })

  it('does not set a lockfile to restore when pnpm-lock.yaml exists', async () => {
    const lastValidatedTimestamp = Date.now() - 10_000
    const mockWorkspaceState: WorkspaceState = {
      lastValidatedTimestamp,
      pnpmfiles: [],
      settings: {
        excludeLinksFromLockfile: false,
        linkWorkspacePackages: true,
        preferWorkspacePackages: true,
      },
      projects: {},
      filteredInstall: false,
    }
    jest.mocked(loadWorkspaceState).mockReturnValue(mockWorkspaceState)
    mockSingleProjectStats({
      wantedLockfileExists: true,
      currentLockfileMtime: lastValidatedTimestamp - 10_000,
      manifestMtime: lastValidatedTimestamp - 20_000,
    })

    const opts: CheckDepsStatusOptions = {
      rootProjectManifest: {},
      rootProjectManifestDir: '/project',
      pnpmfile: [],
      ...mockWorkspaceState.settings,
    }
    const result = await checkDepsStatus(opts)

    expect(result.upToDate).toBe(true)
    expect(result.wantedLockfileToRestore).toBeUndefined()
  })

  it('still reports the lockfile as not found when the current lockfile is missing too', async () => {
    const lastValidatedTimestamp = Date.now() - 10_000
    const mockWorkspaceState: WorkspaceState = {
      lastValidatedTimestamp,
      pnpmfiles: [],
      settings: {
        excludeLinksFromLockfile: false,
        linkWorkspacePackages: true,
        preferWorkspacePackages: true,
      },
      projects: {},
      filteredInstall: false,
    }
    jest.mocked(loadWorkspaceState).mockReturnValue(mockWorkspaceState)
    jest.mocked(fsUtils.safeStat).mockResolvedValue(undefined)
    jest.mocked(fsUtils.safeStatSync).mockReturnValue(undefined)
    jest.mocked(statManifestFileUtils.statManifestFile).mockResolvedValue(undefined)
    jest.mocked(lockfileFs.readCurrentLockfile).mockResolvedValue(null)
    jest.mocked(lockfileFs.readWantedLockfile).mockResolvedValue(null)

    const opts: CheckDepsStatusOptions = {
      rootProjectManifest: {},
      rootProjectManifestDir: '/project',
      pnpmfile: [],
      ...mockWorkspaceState.settings,
    }
    const result = await checkDepsStatus(opts)

    expect(result.upToDate).toBe(false)
    expect(result.issue).toMatch(/Cannot find a lockfile/)
    expect(result.wantedLockfileToRestore).toBeUndefined()
  })

  it('does not stand in for the wanted lockfile when git-branch lockfiles are enabled', async () => {
    const lastValidatedTimestamp = Date.now() - 10_000
    const mockWorkspaceState: WorkspaceState = {
      lastValidatedTimestamp,
      pnpmfiles: [],
      settings: {
        excludeLinksFromLockfile: false,
        linkWorkspacePackages: true,
        preferWorkspacePackages: true,
      },
      projects: {},
      filteredInstall: false,
    }
    jest.mocked(loadWorkspaceState).mockReturnValue(mockWorkspaceState)
    mockSingleProjectStats({
      wantedLockfileExists: false,
      currentLockfileMtime: lastValidatedTimestamp - 10_000,
      manifestMtime: lastValidatedTimestamp - 20_000,
    })

    const opts: CheckDepsStatusOptions = {
      rootProjectManifest: {},
      rootProjectManifestDir: '/project',
      pnpmfile: [],
      useGitBranchLockfile: true,
      ...mockWorkspaceState.settings,
    }
    const result = await checkDepsStatus(opts)

    expect(result.upToDate).toBe(false)
    expect(result.issue).toMatch(/Cannot find a lockfile/)
    expect(result.wantedLockfileToRestore).toBeUndefined()
  })

  it('returns the current lockfile to restore for a workspace with unmodified manifests', async () => {
    const lastValidatedTimestamp = Date.now() - 10_000
    const projectRootDir = '/workspace' as ProjectRootDir
    const mockWorkspaceState: WorkspaceState = {
      lastValidatedTimestamp,
      pnpmfiles: [],
      settings: {
        excludeLinksFromLockfile: false,
        linkWorkspacePackages: true,
        preferWorkspacePackages: true,
      },
      projects: {
        [projectRootDir]: { name: 'root', version: '1.0.0' },
      },
      filteredInstall: false,
    }
    jest.mocked(loadWorkspaceState).mockReturnValue(mockWorkspaceState)
    jest.mocked(fsUtils.safeStat).mockResolvedValue(undefined)
    jest.mocked(fsUtils.safeStatSync).mockReturnValue(undefined)
    jest.mocked(statManifestFileUtils.statManifestFile).mockImplementation(async () => ({
      mtime: new Date(lastValidatedTimestamp - 20_000),
      mtimeMs: lastValidatedTimestamp - 20_000,
    } as Stats))
    jest.mocked(lockfileFs.readCurrentLockfile).mockImplementation(async () => currentLockfile)
    jest.mocked(lockfileFs.readWantedLockfile).mockResolvedValue(null)

    const opts: CheckDepsStatusOptions = {
      allProjects: [
        {
          rootDir: projectRootDir,
          rootDirRealPath: '/workspace' as ProjectRootDirRealPath,
          manifest: { name: 'root', version: '1.0.0' },
          writeProjectManifest: async () => {},
        },
      ],
      workspaceDir: '/workspace',
      sharedWorkspaceLockfile: true,
      rootProjectManifest: { name: 'root', version: '1.0.0' },
      rootProjectManifestDir: '/workspace',
      pnpmfile: [],
      ...mockWorkspaceState.settings,
    }
    const result = await checkDepsStatus(opts)

    expect(result.upToDate).toBe(true)
    expect(result.wantedLockfileToRestore).toEqual({
      lockfile: currentLockfile,
      lockfileDir: '/workspace',
    })
  })
})

describe('checkDepsStatus - treatLocalFileDepsAsOutdated', () => {
  beforeEach(() => {
    jest.resetModules()
    jest.clearAllMocks()
  })

  const currentLockfile = {
    lockfileVersion: '9.0',
    importers: { '.': { specifiers: {} } },
  } as unknown as LockfileObject

  const mockWorkspaceState = (lastValidatedTimestamp: number): WorkspaceState => ({
    lastValidatedTimestamp,
    pnpmfiles: [],
    settings: {
      excludeLinksFromLockfile: false,
      linkWorkspacePackages: true,
      preferWorkspacePackages: true,
    },
    projects: {},
    filteredInstall: false,
  })

  function mockUpToDateSingleProjectStats (lastValidatedTimestamp: number): void {
    jest.mocked(fsUtils.safeStat).mockImplementation(async () => ({
      mtime: new Date(lastValidatedTimestamp - 10_000),
      mtimeMs: lastValidatedTimestamp - 10_000,
    } as Stats))
    jest.mocked(fsUtils.safeStatSync).mockReturnValue(undefined)
    jest.mocked(statManifestFileUtils.statManifestFile).mockImplementation(async () => ({
      mtime: new Date(lastValidatedTimestamp - 20_000),
      mtimeMs: lastValidatedTimestamp - 20_000,
    } as Stats))
    jest.mocked(lockfileFs.readCurrentLockfile).mockImplementation(async () => currentLockfile)
    jest.mocked(lockfileFs.readWantedLockfile).mockResolvedValue(null)
  }

  it('returns upToDate: false when the root manifest has a file: dependency', async () => {
    const lastValidatedTimestamp = Date.now() - 10_000
    jest.mocked(loadWorkspaceState).mockReturnValue(mockWorkspaceState(lastValidatedTimestamp))

    const opts: CheckDepsStatusOptions = {
      rootProjectManifest: {
        dependencies: { foo: 'file:../foo' },
      },
      rootProjectManifestDir: '/project',
      pnpmfile: [],
      treatLocalFileDepsAsOutdated: true,
      ...mockWorkspaceState(lastValidatedTimestamp).settings,
    }
    const result = await checkDepsStatus(opts)

    expect(result.upToDate).toBe(false)
    expect(result.issue).toBe('The dependency "foo" is a local file dependency and its contents may have changed')
  })

  it('returns upToDate: false when a workspace project has a file: dependency', async () => {
    const lastValidatedTimestamp = Date.now() - 10_000
    jest.mocked(loadWorkspaceState).mockReturnValue(mockWorkspaceState(lastValidatedTimestamp))

    const opts: CheckDepsStatusOptions = {
      allProjects: [
        {
          rootDir: '/workspace' as ProjectRootDir,
          rootDirRealPath: '/workspace' as ProjectRootDirRealPath,
          manifest: { name: 'root', version: '1.0.0' },
          writeProjectManifest: async () => {},
        },
        {
          rootDir: '/workspace/packages/bar' as ProjectRootDir,
          rootDirRealPath: '/workspace/packages/bar' as ProjectRootDirRealPath,
          manifest: {
            name: 'bar',
            version: '1.0.0',
            devDependencies: { tar: 'file:./vendor/tar.tgz' },
          },
          writeProjectManifest: async () => {},
        },
      ],
      workspaceDir: '/workspace',
      sharedWorkspaceLockfile: true,
      rootProjectManifest: { name: 'root', version: '1.0.0' },
      rootProjectManifestDir: '/workspace',
      pnpmfile: [],
      treatLocalFileDepsAsOutdated: true,
      ...mockWorkspaceState(lastValidatedTimestamp).settings,
    }
    const result = await checkDepsStatus(opts)

    expect(result.upToDate).toBe(false)
    expect(result.issue).toBe('The dependency "tar" is a local file dependency and its contents may have changed')
  })

  it('reports up-to-date when there is a file: dependency but the option is not set', async () => {
    const lastValidatedTimestamp = Date.now() - 10_000
    jest.mocked(loadWorkspaceState).mockReturnValue(mockWorkspaceState(lastValidatedTimestamp))
    mockUpToDateSingleProjectStats(lastValidatedTimestamp)

    const opts: CheckDepsStatusOptions = {
      rootProjectManifest: {
        dependencies: { foo: 'file:../foo' },
      },
      rootProjectManifestDir: '/project',
      pnpmfile: [],
      ...mockWorkspaceState(lastValidatedTimestamp).settings,
    }
    const result = await checkDepsStatus(opts)

    expect(result.upToDate).toBe(true)
  })

  it('does not report link: and registry dependencies as outdated', async () => {
    const lastValidatedTimestamp = Date.now() - 10_000
    jest.mocked(loadWorkspaceState).mockReturnValue(mockWorkspaceState(lastValidatedTimestamp))
    mockUpToDateSingleProjectStats(lastValidatedTimestamp)

    const opts: CheckDepsStatusOptions = {
      rootProjectManifest: {
        dependencies: {
          foo: 'link:../foo',
          bar: '^1.0.0',
        },
      },
      rootProjectManifestDir: '/project',
      pnpmfile: [],
      treatLocalFileDepsAsOutdated: true,
      ...mockWorkspaceState(lastValidatedTimestamp).settings,
    }
    const result = await checkDepsStatus(opts)

    expect(result.upToDate).toBe(true)
  })

  it.each([
    ['a bare local tarball path', 'vendor/pkg.tgz'],
    ['a relative directory path', '../sibling-dir'],
    ['a home-relative path', '~/pkgs/foo'],
    ['an absolute path', '/abs/path/foo'],
  ])('returns upToDate: false when the root manifest has %s dependency', async (_desc, spec) => {
    const lastValidatedTimestamp = Date.now() - 10_000
    jest.mocked(loadWorkspaceState).mockReturnValue(mockWorkspaceState(lastValidatedTimestamp))

    const opts: CheckDepsStatusOptions = {
      rootProjectManifest: {
        dependencies: { foo: spec },
      },
      rootProjectManifestDir: '/project',
      pnpmfile: [],
      treatLocalFileDepsAsOutdated: true,
      ...mockWorkspaceState(lastValidatedTimestamp).settings,
    }
    const result = await checkDepsStatus(opts)

    expect(result.upToDate).toBe(false)
    expect(result.issue).toBe('The dependency "foo" is a local file dependency and its contents may have changed')
  })

  it('does not report git, remote tarball, and tilde range dependencies as outdated', async () => {
    const lastValidatedTimestamp = Date.now() - 10_000
    jest.mocked(loadWorkspaceState).mockReturnValue(mockWorkspaceState(lastValidatedTimestamp))
    mockUpToDateSingleProjectStats(lastValidatedTimestamp)

    const opts: CheckDepsStatusOptions = {
      rootProjectManifest: {
        dependencies: {
          foo: 'user/repo',
          bar: 'github:user/repo',
          baz: 'https://example.com/pkg.tgz',
          qux: '~1.2.3',
        },
      },
      rootProjectManifestDir: '/project',
      pnpmfile: [],
      treatLocalFileDepsAsOutdated: true,
      ...mockWorkspaceState(lastValidatedTimestamp).settings,
    }
    const result = await checkDepsStatus(opts)

    expect(result.upToDate).toBe(true)
  })
})
