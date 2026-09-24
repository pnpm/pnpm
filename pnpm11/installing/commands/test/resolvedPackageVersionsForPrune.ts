import fs from 'node:fs'
import os from 'node:os'
import path from 'node:path'

import { expect, jest, test } from '@jest/globals'
import type { DepPath } from '@pnpm/types'

const actualLockfileFs = await import('@pnpm/lockfile.fs')

const mockReadWantedLockfile = jest.fn(actualLockfileFs.readWantedLockfile)

jest.unstable_mockModule('@pnpm/lockfile.fs', () => ({
  ...actualLockfileFs,
  readWantedLockfile: mockReadWantedLockfile,
}))

const { resolvedPackageVersionsForPrune, resolvedPackageVersionsOfProjectLockfiles } =
  await import('../src/resolvedPackageVersionsForPrune.js')

const newLockfile = {
  importers: {},
  lockfileVersion: '9.0',
  packages: {
    ['foo@1.0.0' as DepPath]: { resolution: { integrity: 'AAA' } },
  },
}

test('the versions of the freshly resolved lockfile', () => {
  expect(resolvedPackageVersionsForPrune({}, newLockfile))
    .toEqual(new Map([['foo', new Set(['1.0.0'])]]))
})

test('no versions when the lockfile is not used', () => {
  expect(resolvedPackageVersionsForPrune({
    lockfile: false,
  }, newLockfile)).toBeUndefined()
})

test('no versions when the lockfile is not shared by the whole workspace', () => {
  expect(resolvedPackageVersionsForPrune({
    sharedWorkspaceLockfile: false,
  }, newLockfile)).toBeUndefined()
})

test('no versions when no lockfile is available', () => {
  expect(resolvedPackageVersionsForPrune({}, undefined)).toBeUndefined()
})

test('the versions every project lockfile records together', async () => {
  await withWorkspaceDir(async (workspaceDir) => {
    const projectDirs = writeProjectLockfiles(workspaceDir, { a: 'foo@1.0.0', b: 'foo@2.0.0' })

    expect(await resolvedPackageVersionsOfProjectLockfiles({}, projectDirs))
      .toEqual(new Map([['foo', new Set(['1.0.0', '2.0.0'])]]))
  })
})

test('no versions when a project has no lockfile', async () => {
  await withWorkspaceDir(async (workspaceDir) => {
    const projectDirs = writeProjectLockfiles(workspaceDir, { a: 'foo@1.0.0' })
    const projectWithoutLockfile = path.join(workspaceDir, 'b')
    fs.mkdirSync(projectWithoutLockfile)

    expect(await resolvedPackageVersionsOfProjectLockfiles({}, [...projectDirs, projectWithoutLockfile])).toBeUndefined()
  })
})

test('the versions recorded in a branch lockfile when useGitBranchLockfile is enabled', async () => {
  mockReadWantedLockfile.mockImplementationOnce(async (dir, opts) => {
    expect(opts).toMatchObject({
      ignoreIncompatible: true,
      useGitBranchLockfile: true,
    })
    return {
      importers: {},
      lockfileVersion: '9.0',
      packages: {
        ['foo@3.0.0' as DepPath]: { resolution: { integrity: 'AAA' } },
      },
      snapshots: {
        ['foo@3.0.0' as DepPath]: {},
      },
    }
  })

  expect(await resolvedPackageVersionsOfProjectLockfiles({ useGitBranchLockfile: true }, ['/some/project']))
    .toEqual(new Map([['foo', new Set(['3.0.0'])]]))
})

test('the versions recorded across branch lockfiles when mergeGitBranchLockfiles is enabled', async () => {
  mockReadWantedLockfile.mockImplementationOnce(async (dir, opts) => {
    expect(opts).toMatchObject({
      ignoreIncompatible: true,
      useGitBranchLockfile: true,
      mergeGitBranchLockfiles: true,
    })
    return {
      importers: {},
      lockfileVersion: '9.0',
      packages: {
        ['foo@1.0.0' as DepPath]: { resolution: { integrity: 'AAA' } },
        ['bar@2.0.0' as DepPath]: { resolution: { integrity: 'BBB' } },
      },
      snapshots: {
        ['foo@1.0.0' as DepPath]: {},
        ['bar@2.0.0' as DepPath]: {},
      },
    }
  })

  expect(await resolvedPackageVersionsOfProjectLockfiles({ useGitBranchLockfile: true, mergeGitBranchLockfiles: true }, ['/some/project']))
    .toEqual(new Map([
      ['foo', new Set(['1.0.0'])],
      ['bar', new Set(['2.0.0'])],
    ]))
})

async function withWorkspaceDir (fn: (workspaceDir: string) => Promise<void>): Promise<void> {
  const workspaceDir = fs.mkdtempSync(path.join(os.tmpdir(), 'prune-'))
  try {
    await fn(workspaceDir)
  } finally {
    fs.rmSync(workspaceDir, { recursive: true, force: true })
  }
}

function writeProjectLockfiles (workspaceDir: string, snapshots: Record<string, string>): string[] {
  return Object.entries(snapshots).map(([name, snapshot]) => {
    const projectDir = path.join(workspaceDir, name)
    fs.mkdirSync(projectDir)
    fs.writeFileSync(
      path.join(projectDir, 'pnpm-lock.yaml'),
      `lockfileVersion: '9.0'\nimporters:\n  .: {}\npackages:\n  ${snapshot}:\n    resolution: {integrity: AAA}\nsnapshots:\n  ${snapshot}: {}\n`
    )
    return projectDir
  })
}
