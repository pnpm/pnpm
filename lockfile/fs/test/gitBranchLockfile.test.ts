import path from 'node:path'

import { expect, test } from '@jest/globals'

import { getGitBranchLockfileNames } from '../lib/gitBranchLockfile.js'

process.chdir(import.meta.dirname)

test('getGitBranchLockfileNames()', async () => {
  const lockfileDir: string = path.join('fixtures', '6')
  const gitBranchLockfileNames = await getGitBranchLockfileNames(lockfileDir)
  expect(gitBranchLockfileNames).toEqual(['pnpm-lock.branch.yaml'])
})
