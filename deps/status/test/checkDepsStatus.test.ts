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
