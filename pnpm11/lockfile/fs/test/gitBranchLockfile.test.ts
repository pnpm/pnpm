import fs from 'node:fs'
import os from 'node:os'
import path from 'node:path'

import { expect, test } from '@jest/globals'

import { getGitBranchLockfileNames, getGitBranchLockfileNamesSync } from '../lib/gitBranchLockfile.js'

process.chdir(import.meta.dirname)

test('getGitBranchLockfileNames()', async () => {
  const lockfileDir: string = path.join('fixtures', '6')
  const gitBranchLockfileNames = await getGitBranchLockfileNames(lockfileDir)
  expect(gitBranchLockfileNames).toEqual(['pnpm-lock.branch.yaml'])
})

test('getGitBranchLockfileNamesSync()', () => {
  const lockfileDir: string = path.join('fixtures', '6')
  expect(getGitBranchLockfileNamesSync(lockfileDir)).toEqual(['pnpm-lock.branch.yaml'])
})

test('git-branch lockfile matcher requires literal dots and a branch segment', () => {
  const lockfileDir = fs.mkdtempSync(path.join(os.tmpdir(), 'pnpm-git-branch-lockfile-'))
  try {
    for (const name of [
      'pnpm-lock.main.yaml', // branch lockfile
      'pnpm-lock.feature.x.yaml', // branch name containing a dot
      'pnpm-lock.yaml', // base lockfile, not a branch lockfile
      'pnpm-lock-main-yaml', // no literal dots
      'my-pnpm-lock.main.yaml', // does not start at the beginning
      'README.md',
    ]) {
      fs.writeFileSync(path.join(lockfileDir, name), '')
    }
    expect(getGitBranchLockfileNamesSync(lockfileDir).sort()).toEqual([
      'pnpm-lock.feature.x.yaml',
      'pnpm-lock.main.yaml',
    ])
  } finally {
    fs.rmSync(lockfileDir, { force: true, recursive: true })
  }
})
