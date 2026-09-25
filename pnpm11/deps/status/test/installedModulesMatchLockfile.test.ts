import type { Stats } from 'node:fs'

import { beforeEach, describe, expect, it, jest } from '@jest/globals'
import { LOCKFILE_VERSION } from '@pnpm/constants'
import type { CheckDepsStatusOptions } from '@pnpm/deps.status'
import type { LockfileObject } from '@pnpm/lockfile.fs'
import type { DepPath, Project, ProjectId, ProjectRootDir } from '@pnpm/types'

{
  const original = await import('../lib/safeStat.js')
  jest.unstable_mockModule('../lib/safeStat.js', () => ({
    ...original,
    safeStat: jest.fn(),
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

const { installedModulesMatchLockfile } = await import('../lib/installedModulesMatchLockfile.js')
const lockfileFs = await import('@pnpm/lockfile.fs')
const fsUtils = await import('../lib/safeStat.js')

describe('installedModulesMatchLockfile', () => {
  beforeEach(() => {
    jest.resetModules()
    jest.clearAllMocks()
  })

  function makeLockfile (overrides?: Record<string, string>): LockfileObject {
    return {
      lockfileVersion: LOCKFILE_VERSION,
      importers: {
        ['.' as ProjectId]: {
          specifiers: {
            foo: '^1.0.0',
          },
          dependencies: {
            foo: '1.0.0',
          },
        },
      },
      overrides,
    }
  }

  const manifest = {
    name: 'test-project',
    version: '1.0.0',
    dependencies: {
      foo: '^1.0.0',
    },
  }

  const baseOpts: Partial<CheckDepsStatusOptions> = {
    excludeLinksFromLockfile: false,
    linkWorkspacePackages: true,
  }

  it('returns false when no projects can be discovered', async () => {
    const result = await installedModulesMatchLockfile({
      ...baseOpts,
      rootProjectManifest: undefined,
      rootProjectManifestDir: undefined as unknown as string,
    } as CheckDepsStatusOptions)
    expect(result).toBe(false)
  })

  it('returns true when lockfiles and manifests match and node_modules exists', async () => {
    const lockfile = makeLockfile()
    jest.mocked(lockfileFs.readWantedLockfile).mockResolvedValue(lockfile)
    jest.mocked(lockfileFs.readCurrentLockfile).mockResolvedValue(lockfile)
    jest.mocked(fsUtils.safeStat).mockResolvedValue({
      isDirectory: () => true,
    } as Stats)

    const result = await installedModulesMatchLockfile({
      ...baseOpts,
      rootProjectManifest: manifest,
      rootProjectManifestDir: '/workspace' as ProjectRootDir,
      lockfileDir: '/workspace',
    } as CheckDepsStatusOptions)
    expect(result).toBe(true)
  })

  it('returns false when wanted and current lockfiles differ', async () => {
    const wanted = makeLockfile()
    const current: LockfileObject = {
      ...makeLockfile(),
      importers: {
        ['.' as ProjectId]: {
          specifiers: { foo: '^2.0.0' },
          dependencies: { foo: '2.0.0' },
        },
      },
    }
    jest.mocked(lockfileFs.readWantedLockfile).mockResolvedValue(wanted)
    jest.mocked(lockfileFs.readCurrentLockfile).mockResolvedValue(current)

    const result = await installedModulesMatchLockfile({
      ...baseOpts,
      rootProjectManifest: manifest,
      rootProjectManifestDir: '/workspace' as ProjectRootDir,
      lockfileDir: '/workspace',
    } as CheckDepsStatusOptions)
    expect(result).toBe(false)
  })

  it('returns false when lockfile settings (overrides) have changed', async () => {
    const lockfile = makeLockfile({ foo: '1.0.0' })
    jest.mocked(lockfileFs.readWantedLockfile).mockResolvedValue(lockfile)
    jest.mocked(lockfileFs.readCurrentLockfile).mockResolvedValue(lockfile)
    jest.mocked(fsUtils.safeStat).mockResolvedValue({
      isDirectory: () => true,
    } as Stats)

    const result = await installedModulesMatchLockfile({
      ...baseOpts,
      rootProjectManifest: manifest,
      rootProjectManifestDir: '/workspace' as ProjectRootDir,
      lockfileDir: '/workspace',
      overrides: { foo: '2.0.0' },
    } as CheckDepsStatusOptions)
    expect(result).toBe(false)
  })

  it('returns false when project node_modules directory does not exist', async () => {
    const lockfile = makeLockfile()
    jest.mocked(lockfileFs.readWantedLockfile).mockResolvedValue(lockfile)
    jest.mocked(lockfileFs.readCurrentLockfile).mockResolvedValue(lockfile)
    jest.mocked(fsUtils.safeStat).mockResolvedValue(undefined)

    const result = await installedModulesMatchLockfile({
      ...baseOpts,
      rootProjectManifest: manifest,
      rootProjectManifestDir: '/workspace' as ProjectRootDir,
      lockfileDir: '/workspace',
    } as CheckDepsStatusOptions)
    expect(result).toBe(false)
  })

  it('checks separate lockfiles when sharedWorkspaceLockfile is false', async () => {
    const lockfile = makeLockfile()
    jest.mocked(lockfileFs.readWantedLockfile).mockResolvedValue(lockfile)
    jest.mocked(lockfileFs.readCurrentLockfile).mockResolvedValue(lockfile)
    jest.mocked(fsUtils.safeStat).mockResolvedValue({
      isDirectory: () => true,
    } as Stats)

    const result = await installedModulesMatchLockfile({
      ...baseOpts,
      rootProjectManifest: manifest,
      rootProjectManifestDir: '/workspace/project-a' as ProjectRootDir,
      sharedWorkspaceLockfile: false,
    } as CheckDepsStatusOptions)
    expect(result).toBe(true)
    expect(lockfileFs.readWantedLockfile).toHaveBeenCalledWith(
      '/workspace/project-a',
      expect.anything()
    )
  })

  it('includes rootProjectManifest when allProjects does not contain rootProjectManifestDir', async () => {
    const childLockfile: LockfileObject = {
      lockfileVersion: LOCKFILE_VERSION,
      importers: {
        ['.' as ProjectId]: {
          specifiers: { foo: '^1.0.0' },
          dependencies: { foo: '1.0.0' },
        },
        ['packages/child' as ProjectId]: {
          specifiers: { bar: '^1.0.0' },
          dependencies: { bar: '1.0.0' },
        },
      },
    }
    jest.mocked(lockfileFs.readWantedLockfile).mockResolvedValue(childLockfile)
    jest.mocked(lockfileFs.readCurrentLockfile).mockResolvedValue(childLockfile)
    jest.mocked(fsUtils.safeStat).mockResolvedValue({
      isDirectory: () => true,
    } as Stats)

    const childProject: Partial<Project> = {
      manifest: {
        name: 'child-project',
        version: '1.0.0',
        dependencies: { bar: '^1.0.0' },
      },
      rootDir: '/workspace/packages/child' as ProjectRootDir,
    }

    const result = await installedModulesMatchLockfile({
      ...baseOpts,
      rootProjectManifest: manifest,
      rootProjectManifestDir: '/workspace' as ProjectRootDir,
      lockfileDir: '/workspace',
      allProjects: [childProject as Project],
    } as CheckDepsStatusOptions)
    expect(result).toBe(true)
  })

  it('returns false when wanted lockfile has stale patched dependency paths', async () => {
    const lockfile: LockfileObject = {
      ...makeLockfile(),
      packages: {
        ['is-positive@1.0.0(patch_hash=bbbb2222)' as DepPath]: {
          resolution: { integrity: 'sha512-fake' },
        },
      },
    }
    jest.mocked(lockfileFs.readWantedLockfile).mockResolvedValue(lockfile)
    jest.mocked(lockfileFs.readCurrentLockfile).mockResolvedValue(lockfile)
    jest.mocked(fsUtils.safeStat).mockResolvedValue({
      isDirectory: () => true,
    } as Stats)

    const result = await installedModulesMatchLockfile({
      ...baseOpts,
      rootProjectManifest: manifest,
      rootProjectManifestDir: '/workspace' as ProjectRootDir,
      lockfileDir: '/workspace',
    } as CheckDepsStatusOptions)
    expect(result).toBe(false)
  })

  it('returns false when wanted lockfile has indeterminate patched dependency paths', async () => {
    const lockfile: LockfileObject = {
      ...makeLockfile(),
      packages: {
        ['foo@1.0.0(patch_hash=1111)(patch_hash=2222)' as DepPath]: {
          resolution: { integrity: 'sha512-fake' },
        },
      },
    }
    jest.mocked(lockfileFs.readWantedLockfile).mockResolvedValue(lockfile)
    jest.mocked(lockfileFs.readCurrentLockfile).mockResolvedValue(lockfile)
    jest.mocked(fsUtils.safeStat).mockResolvedValue({
      isDirectory: () => true,
    } as Stats)

    const result = await installedModulesMatchLockfile({
      ...baseOpts,
      rootProjectManifest: manifest,
      rootProjectManifestDir: '/workspace' as ProjectRootDir,
      lockfileDir: '/workspace',
    } as CheckDepsStatusOptions)
    expect(result).toBe(false)
  })
})
