import fs from 'node:fs'
import os from 'node:os'
import path from 'node:path'

import { expect, test } from '@jest/globals'
import type { DepPath } from '@pnpm/types'

import { resolvedPackageVersionsForPrune, resolvedPackageVersionsOfProjectLockfiles } from '../src/resolvedPackageVersionsForPrune.js'

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
  const workspaceDir = fs.mkdtempSync(path.join(os.tmpdir(), 'prune-'))
  const projectDirs = writeProjectLockfiles(workspaceDir, { a: 'foo@1.0.0', b: 'foo@2.0.0' })

  expect(await resolvedPackageVersionsOfProjectLockfiles({}, projectDirs))
    .toEqual(new Map([['foo', new Set(['1.0.0', '2.0.0'])]]))
})

test('no versions when a project has no lockfile', async () => {
  const workspaceDir = fs.mkdtempSync(path.join(os.tmpdir(), 'prune-'))
  const projectDirs = writeProjectLockfiles(workspaceDir, { a: 'foo@1.0.0' })
  const projectWithoutLockfile = path.join(workspaceDir, 'b')
  fs.mkdirSync(projectWithoutLockfile)

  expect(await resolvedPackageVersionsOfProjectLockfiles({}, [...projectDirs, projectWithoutLockfile])).toBeUndefined()
})

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
