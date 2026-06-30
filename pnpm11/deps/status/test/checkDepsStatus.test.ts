import type { Stats } from 'node:fs'
import fs from 'node:fs/promises'
import os from 'node:os'
import path from 'node:path'

import { beforeEach, describe, expect, it, jest } from '@jest/globals'
import type { CheckDepsStatusOptions } from '@pnpm/deps.status'
import type { LockfileObject } from '@pnpm/lockfile.fs'
import type { ProjectId, ProjectRootDir, ProjectRootDirRealPath } from '@pnpm/types'
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
    getWantedLockfileName: jest.fn(original.getWantedLockfileName),
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

  it('detects merge conflicts in the git-branch lockfile when useGitBranchLockfile is enabled', async () => {
    const projectDir = await fs.mkdtemp(path.join(os.tmpdir(), 'pnpm-check-deps-git-branch-conflict-'))
    try {
      const branchLockfileName = 'pnpm-lock.main.yaml'
      await writeConflictedLockfile(projectDir, branchLockfileName)
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
      jest.mocked(lockfileFs.getWantedLockfileName).mockResolvedValueOnce(branchLockfileName)

      const opts: CheckDepsStatusOptions = {
        rootProjectManifest: {},
        rootProjectManifestDir: projectDir,
        pnpmfile: [],
        useGitBranchLockfile: true,
        ...mockWorkspaceState.settings,
      }
      const result = await checkDepsStatus(opts)

      expect(result.upToDate).toBe(false)
      expect(result.issue).toBe(`The lockfile in ${projectDir} has merge conflicts`)
    } finally {
      await fs.rm(projectDir, { force: true, recursive: true })
    }
  })

  it('detects merge conflicts in a branch lockfile when mergeGitBranchLockfiles is enabled', async () => {
    const projectDir = await fs.mkdtemp(path.join(os.tmpdir(), 'pnpm-check-deps-merge-branch-conflict-'))
    try {
      // The merged wanted lockfile is `pnpm-lock.yaml` + every `pnpm-lock.*.yaml`.
      // Leave `pnpm-lock.yaml` unmodified, but introduce a conflict in a branch
      // lockfile and assert it is still detected.
      const unmodifiedMtime = (Date.now() - 20_000) / 1000
      await fs.writeFile(path.join(projectDir, 'pnpm-lock.yaml'), "lockfileVersion: '9.0'\n")
      await fs.utimes(path.join(projectDir, 'pnpm-lock.yaml'), unmodifiedMtime, unmodifiedMtime)
      await writeConflictedLockfile(projectDir, 'pnpm-lock.feature.yaml')
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
        useGitBranchLockfile: true,
        mergeGitBranchLockfiles: true,
        ...mockWorkspaceState.settings,
      }
      const result = await checkDepsStatus(opts)

      expect(result.upToDate).toBe(false)
      expect(result.issue).toBe(`The lockfile in ${projectDir} has merge conflicts`)
    } finally {
      await fs.rm(projectDir, { force: true, recursive: true })
    }
  })
})

describe('checkDepsStatus - lockfile modification', () => {
  beforeEach(() => {
    jest.resetModules()
    jest.clearAllMocks()
  })

  it('does not skip the wanted lockfile check when only the lockfile changed since the last validation', async () => {
    const workspaceDir = await fs.mkdtemp(path.join(os.tmpdir(), 'pnpm-check-deps-lockfile-'))
    try {
      const lastValidatedTimestamp = Date.now() - 10_000
      const beforeLastValidation = lastValidatedTimestamp - 10_000
      const afterLastValidation = lastValidatedTimestamp + 1_000
      const projectRootDir = workspaceDir as ProjectRootDir
      const projectRootDirRealPath = await fs.realpath(workspaceDir) as ProjectRootDirRealPath
      const lockfile: LockfileObject = {
        lockfileVersion: '9.0',
        importers: {
          ['.' as ProjectId]: {
            specifiers: {},
          },
        },
      }
      const mockWorkspaceState: WorkspaceState = {
        lastValidatedTimestamp,
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

      await fs.writeFile(path.join(workspaceDir, 'pnpm-lock.yaml'), "lockfileVersion: '9.0'\n")

      jest.mocked(loadWorkspaceState).mockReturnValue(mockWorkspaceState)
      jest.mocked(fsUtils.safeStatSync).mockImplementation((filePath: string) => {
        if (filePath === path.join(workspaceDir, 'pnpm-lock.yaml')) {
          return {
            mtime: new Date(afterLastValidation),
            mtimeMs: afterLastValidation,
          } as Stats
        }
        return undefined
      })
      jest.mocked(fsUtils.safeStat).mockImplementation(async (filePath: string) => {
        if (filePath.endsWith('pnpm-lock.yaml')) {
          return {
            mtime: new Date(afterLastValidation),
            mtimeMs: afterLastValidation,
          } as Stats
        }
        return undefined
      })
      jest.mocked(statManifestFileUtils.statManifestFile).mockResolvedValue({
        mtime: new Date(beforeLastValidation),
        mtimeMs: beforeLastValidation,
      } as Stats)
      const wantedLockfile: LockfileObject = {
        lockfileVersion: '9.0',
        importers: {
          ['.' as ProjectId]: {
            specifiers: { foo: '1.0.0' },
          },
        },
      }
      jest.mocked(lockfileFs.readCurrentLockfile).mockResolvedValue(lockfile)
      jest.mocked(lockfileFs.readWantedLockfile).mockResolvedValue(wantedLockfile)

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
        ...mockWorkspaceState.settings,
      }
      const result = await checkDepsStatus(opts)

      expect(result.upToDate).toBe(false)
      expect(result.issue).toBe(`The installed dependencies in the modules directory is not up-to-date with the lockfile in ${workspaceDir}.`)
    } finally {
      await fs.rm(workspaceDir, { force: true, recursive: true })
    }
  })

  it('does not throw when pnpm-lock.yaml is absent but a git-branch lockfile exists', async () => {
    const workspaceDir = await fs.mkdtemp(path.join(os.tmpdir(), 'pnpm-check-deps-git-branch-'))
    try {
      const lastValidatedTimestamp = Date.now() - 10_000
      const beforeLastValidation = lastValidatedTimestamp - 10_000
      const projectRootDir = workspaceDir as ProjectRootDir
      const projectRootDirRealPath = await fs.realpath(workspaceDir) as ProjectRootDirRealPath
      const branchLockfileName = 'pnpm-lock.main.yaml'
      const mockWorkspaceState: WorkspaceState = {
        lastValidatedTimestamp,
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

      await fs.writeFile(path.join(workspaceDir, branchLockfileName), "lockfileVersion: '9.0'\n")
      const branchLockfilePath = path.join(workspaceDir, branchLockfileName)
      await fs.utimes(branchLockfilePath, beforeLastValidation / 1000, beforeLastValidation / 1000)

      jest.mocked(loadWorkspaceState).mockReturnValue(mockWorkspaceState)
      jest.mocked(lockfileFs.getWantedLockfileName).mockResolvedValueOnce(branchLockfileName)
      jest.mocked(fsUtils.safeStatSync).mockReturnValue(undefined)
      jest.mocked(fsUtils.safeStat).mockResolvedValue(undefined)
      jest.mocked(statManifestFileUtils.statManifestFile).mockResolvedValue({
        mtime: new Date(beforeLastValidation),
        mtimeMs: beforeLastValidation,
      } as Stats)

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
        sharedWorkspaceLockfile: true,
        useGitBranchLockfile: true,
        rootProjectManifest: {},
        rootProjectManifestDir: workspaceDir,
        pnpmfile: [],
        ...mockWorkspaceState.settings,
      }
      const result = await checkDepsStatus(opts)

      expect(result.upToDate).toBe(true)
      expect(result.issue).toBeUndefined()
    } finally {
      await fs.rm(workspaceDir, { force: true, recursive: true })
    }
  })

  it('does not take the optimistic fast-path when the git-branch lockfile is missing', async () => {
    const workspaceDir = await fs.mkdtemp(path.join(os.tmpdir(), 'pnpm-check-deps-git-branch-missing-'))
    try {
      const lastValidatedTimestamp = Date.now() - 10_000
      const beforeLastValidation = lastValidatedTimestamp - 10_000
      const projectRootDir = workspaceDir as ProjectRootDir
      const projectRootDirRealPath = await fs.realpath(workspaceDir) as ProjectRootDirRealPath
      const mockWorkspaceState: WorkspaceState = {
        lastValidatedTimestamp,
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

      // No lockfile is written: `pnpm-lock.main.yaml` is missing on disk.
      jest.mocked(loadWorkspaceState).mockReturnValue(mockWorkspaceState)
      jest.mocked(lockfileFs.getWantedLockfileName).mockResolvedValueOnce('pnpm-lock.main.yaml')
      jest.mocked(fsUtils.safeStatSync).mockReturnValue(undefined)
      jest.mocked(fsUtils.safeStat).mockResolvedValue(undefined)
      jest.mocked(statManifestFileUtils.statManifestFile).mockResolvedValue({
        mtime: new Date(beforeLastValidation),
        mtimeMs: beforeLastValidation,
      } as Stats)

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
        sharedWorkspaceLockfile: true,
        useGitBranchLockfile: true,
        rootProjectManifest: {},
        rootProjectManifestDir: workspaceDir,
        pnpmfile: [],
        ...mockWorkspaceState.settings,
      }
      const result = await checkDepsStatus(opts)

      expect(result.upToDate).toBe(false)
      expect(result.issue).toBe(`Cannot find a lockfile in ${workspaceDir}`)
    } finally {
      await fs.rm(workspaceDir, { force: true, recursive: true })
    }
  })

  it('passes the workspace dir as cwd to getWantedLockfileName so git branch is resolved in the correct repo', async () => {
    jest.mocked(lockfileFs.getWantedLockfileName).mockResolvedValueOnce('pnpm-lock.main.yaml')
    jest.mocked(loadWorkspaceState).mockReturnValue({
      lastValidatedTimestamp: Date.now() - 10_000,
      pnpmfiles: [],
      settings: { excludeLinksFromLockfile: false, linkWorkspacePackages: true, preferWorkspacePackages: true },
      projects: {},
      filteredInstall: false,
    })
    jest.mocked(fsUtils.safeStatSync).mockReturnValue(undefined)
    jest.mocked(fsUtils.safeStat).mockResolvedValue(undefined)
    jest.mocked(statManifestFileUtils.statManifestFile).mockResolvedValue(undefined)

    const opts: CheckDepsStatusOptions = {
      allProjects: [{
        rootDir: '/workspace/pkg' as ProjectRootDir,
        rootDirRealPath: '/workspace/pkg' as ProjectRootDirRealPath,
        manifest: { name: 'pkg', version: '1.0.0' },
        writeProjectManifest: async () => {},
      }],
      workspaceDir: '/workspace',
      sharedWorkspaceLockfile: true,
      useGitBranchLockfile: true,
      rootProjectManifest: {},
      rootProjectManifestDir: '/workspace',
      pnpmfile: [],
      excludeLinksFromLockfile: false,
      linkWorkspacePackages: true,
      preferWorkspacePackages: true,
    }
    await checkDepsStatus(opts)

    expect(jest.mocked(lockfileFs.getWantedLockfileName)).toHaveBeenCalledWith({
      useGitBranchLockfile: true,
      mergeGitBranchLockfiles: undefined,
      cwd: '/workspace',
    })
  })
})

async function writeConflictedLockfile (lockfileDir: string, lockfileName: string = 'pnpm-lock.yaml'): Promise<void> {
  await fs.writeFile(path.join(lockfileDir, lockfileName), [
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

  it('returns upToDate: false when the root manifest has a file: dependency but allProjects omits the root', async () => {
    const lastValidatedTimestamp = Date.now() - 10_000
    jest.mocked(loadWorkspaceState).mockReturnValue(mockWorkspaceState(lastValidatedTimestamp))

    const opts: CheckDepsStatusOptions = {
      allProjects: [
        {
          rootDir: '/workspace/packages/bar' as ProjectRootDir,
          rootDirRealPath: '/workspace/packages/bar' as ProjectRootDirRealPath,
          manifest: { name: 'bar', version: '1.0.0' },
          writeProjectManifest: async () => {},
        },
      ],
      workspaceDir: '/workspace',
      sharedWorkspaceLockfile: true,
      rootProjectManifest: {
        name: 'root',
        version: '1.0.0',
        dependencies: { foo: 'file:../foo' },
      },
      rootProjectManifestDir: '/workspace',
      pnpmfile: [],
      treatLocalFileDepsAsOutdated: true,
      ...mockWorkspaceState(lastValidatedTimestamp).settings,
    }
    const result = await checkDepsStatus(opts)

    expect(result.upToDate).toBe(false)
    expect(result.issue).toBe('The dependency "foo" is a local file dependency and its contents may have changed')
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
    ['an absolute Windows drive path', 'C:\\pkgs\\foo'],
    ['a drive-relative Windows path', 'C:pkgs'],
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

  it('returns upToDate: false when an override maps to a local file dependency', async () => {
    const lastValidatedTimestamp = Date.now() - 10_000
    jest.mocked(loadWorkspaceState).mockReturnValue(mockWorkspaceState(lastValidatedTimestamp))

    const opts: CheckDepsStatusOptions = {
      rootProjectManifest: {
        dependencies: { foo: '^1.0.0' },
      },
      rootProjectManifestDir: '/project',
      overrides: { bar: 'file:../bar' },
      pnpmfile: [],
      treatLocalFileDepsAsOutdated: true,
      ...mockWorkspaceState(lastValidatedTimestamp).settings,
    }
    const result = await checkDepsStatus(opts)

    expect(result.upToDate).toBe(false)
    expect(result.issue).toBe('The override "bar" maps to a local file dependency and its contents may have changed')
  })

  it('does not report registry and link: overrides as outdated', async () => {
    const lastValidatedTimestamp = Date.now() - 10_000
    const overrides = { bar: '^2.0.0', baz: 'link:../baz' }
    const workspaceState = mockWorkspaceState(lastValidatedTimestamp)
    workspaceState.settings = { ...workspaceState.settings, overrides }
    jest.mocked(loadWorkspaceState).mockReturnValue(workspaceState)
    mockUpToDateSingleProjectStats(lastValidatedTimestamp)

    const opts: CheckDepsStatusOptions = {
      rootProjectManifest: {
        dependencies: { foo: '^1.0.0' },
      },
      rootProjectManifestDir: '/project',
      pnpmfile: [],
      treatLocalFileDepsAsOutdated: true,
      ...workspaceState.settings,
    }
    const result = await checkDepsStatus(opts)

    expect(result.upToDate).toBe(true)
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
          // A git shorthand whose committish ends in .tgz must not be
          // mistaken for a local tarball.
          quux: 'user/repo#release.tgz',
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

  it('returns upToDate: false for a local file dependency even when nodeLinker is pnp', async () => {
    const lastValidatedTimestamp = Date.now() - 10_000
    jest.mocked(loadWorkspaceState).mockReturnValue(mockWorkspaceState(lastValidatedTimestamp))

    const opts: CheckDepsStatusOptions = {
      rootProjectManifest: {
        dependencies: { foo: 'file:../foo' },
      },
      rootProjectManifestDir: '/project',
      nodeLinker: 'pnp',
      pnpmfile: [],
      treatLocalFileDepsAsOutdated: true,
      ...mockWorkspaceState(lastValidatedTimestamp).settings,
    }
    const result = await checkDepsStatus(opts)

    expect(result.upToDate).toBe(false)
    expect(result.issue).toBe('The dependency "foo" is a local file dependency and its contents may have changed')
  })

  it('reports up-to-date when the only file: dependency is in a group excluded from the install', async () => {
    const lastValidatedTimestamp = Date.now() - 10_000
    jest.mocked(loadWorkspaceState).mockReturnValue(mockWorkspaceState(lastValidatedTimestamp))
    mockUpToDateSingleProjectStats(lastValidatedTimestamp)

    const opts: CheckDepsStatusOptions = {
      rootProjectManifest: {
        optionalDependencies: { foo: 'file:../foo' },
      },
      rootProjectManifestDir: '/project',
      pnpmfile: [],
      treatLocalFileDepsAsOutdated: true,
      include: {
        dependencies: true,
        devDependencies: true,
        optionalDependencies: false,
      },
      ...mockWorkspaceState(lastValidatedTimestamp).settings,
    }
    const result = await checkDepsStatus(opts)

    expect(result.upToDate).toBe(true)
  })

  it('returns upToDate: false for a file: dependency in a group included in the install', async () => {
    const lastValidatedTimestamp = Date.now() - 10_000
    jest.mocked(loadWorkspaceState).mockReturnValue(mockWorkspaceState(lastValidatedTimestamp))

    const opts: CheckDepsStatusOptions = {
      rootProjectManifest: {
        dependencies: { foo: 'file:../foo' },
      },
      rootProjectManifestDir: '/project',
      pnpmfile: [],
      treatLocalFileDepsAsOutdated: true,
      include: {
        dependencies: true,
        devDependencies: false,
        optionalDependencies: false,
      },
      ...mockWorkspaceState(lastValidatedTimestamp).settings,
    }
    const result = await checkDepsStatus(opts)

    expect(result.upToDate).toBe(false)
    expect(result.issue).toBe('The dependency "foo" is a local file dependency and its contents may have changed')
  })

  it('skips non-string dependency specs in malformed manifests without throwing', async () => {
    const lastValidatedTimestamp = Date.now() - 10_000
    jest.mocked(loadWorkspaceState).mockReturnValue(mockWorkspaceState(lastValidatedTimestamp))

    const opts: CheckDepsStatusOptions = {
      rootProjectManifest: {
        dependencies: {
          broken: 42 as unknown as string,
          foo: 'file:../foo',
        },
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

  it('returns upToDate: false when a catalog: dependency resolves to a bare local path', async () => {
    const lastValidatedTimestamp = Date.now() - 10_000
    jest.mocked(loadWorkspaceState).mockReturnValue(mockWorkspaceState(lastValidatedTimestamp))

    const opts: CheckDepsStatusOptions = {
      rootProjectManifest: {
        dependencies: { foo: 'catalog:' },
      },
      rootProjectManifestDir: '/project',
      catalogs: { default: { foo: '../foo' } },
      pnpmfile: [],
      treatLocalFileDepsAsOutdated: true,
      ...mockWorkspaceState(lastValidatedTimestamp).settings,
    }
    const result = await checkDepsStatus(opts)

    expect(result.upToDate).toBe(false)
    expect(result.issue).toBe('The dependency "foo" is a local file dependency and its contents may have changed')
  })

  it('does not report a catalog: dependency resolving to a registry range as outdated', async () => {
    const lastValidatedTimestamp = Date.now() - 10_000
    const catalogs = { default: { foo: '^1.0.0' } }
    const workspaceState = mockWorkspaceState(lastValidatedTimestamp)
    workspaceState.settings = { ...workspaceState.settings, catalogs }
    jest.mocked(loadWorkspaceState).mockReturnValue(workspaceState)
    mockUpToDateSingleProjectStats(lastValidatedTimestamp)

    const opts: CheckDepsStatusOptions = {
      rootProjectManifest: {
        dependencies: { foo: 'catalog:' },
      },
      rootProjectManifestDir: '/project',
      catalogs,
      pnpmfile: [],
      treatLocalFileDepsAsOutdated: true,
      ...workspaceState.settings,
    }
    const result = await checkDepsStatus(opts)

    expect(result.upToDate).toBe(true)
  })

  it('returns upToDate: false when an override maps through a catalog to a local path', async () => {
    const lastValidatedTimestamp = Date.now() - 10_000
    jest.mocked(loadWorkspaceState).mockReturnValue(mockWorkspaceState(lastValidatedTimestamp))

    const opts: CheckDepsStatusOptions = {
      rootProjectManifest: {
        dependencies: { foo: '^1.0.0' },
      },
      rootProjectManifestDir: '/project',
      overrides: { bar: 'catalog:' },
      catalogs: { default: { bar: './vendor/bar' } },
      pnpmfile: [],
      treatLocalFileDepsAsOutdated: true,
      ...mockWorkspaceState(lastValidatedTimestamp).settings,
    }
    const result = await checkDepsStatus(opts)

    expect(result.upToDate).toBe(false)
    expect(result.issue).toBe('The override "bar" maps to a local file dependency and its contents may have changed')
  })

  it('returns upToDate: false when a packageExtension injects a local file dependency', async () => {
    const lastValidatedTimestamp = Date.now() - 10_000
    jest.mocked(loadWorkspaceState).mockReturnValue(mockWorkspaceState(lastValidatedTimestamp))

    const opts: CheckDepsStatusOptions = {
      rootProjectManifest: {
        dependencies: { foo: '^1.0.0' },
      },
      rootProjectManifestDir: '/project',
      packageExtensions: {
        'foo@1': { dependencies: { bar: 'file:../bar' } },
      },
      pnpmfile: [],
      treatLocalFileDepsAsOutdated: true,
      ...mockWorkspaceState(lastValidatedTimestamp).settings,
    }
    const result = await checkDepsStatus(opts)

    expect(result.upToDate).toBe(false)
    expect(result.issue).toBe('The package extension "foo@1" injects a local file dependency and its contents may have changed')
  })

  it('does not report a packageExtension optionalDependency as outdated when optionals are excluded', async () => {
    const lastValidatedTimestamp = Date.now() - 10_000
    const packageExtensions = { 'foo@1': { optionalDependencies: { bar: 'file:../bar' } } }
    const workspaceState = mockWorkspaceState(lastValidatedTimestamp)
    workspaceState.settings = { ...workspaceState.settings, packageExtensions }
    jest.mocked(loadWorkspaceState).mockReturnValue(workspaceState)
    mockUpToDateSingleProjectStats(lastValidatedTimestamp)

    const opts: CheckDepsStatusOptions = {
      rootProjectManifest: {
        dependencies: { foo: '^1.0.0' },
      },
      rootProjectManifestDir: '/project',
      packageExtensions,
      pnpmfile: [],
      treatLocalFileDepsAsOutdated: true,
      include: {
        dependencies: true,
        devDependencies: true,
        optionalDependencies: false,
      },
      ...workspaceState.settings,
    }
    const result = await checkDepsStatus(opts)

    expect(result.upToDate).toBe(true)
  })
})
