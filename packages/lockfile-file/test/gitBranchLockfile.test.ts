import path from 'path'
import { getGitBranchLockfileNames } from '../lib/gitBranchLockfile'

process.chdir(__dirname)

test('getGitBranchLockfileNames()', async () => {
  const lockfileDir: string = path.join('fixtures', '6')
  const gitBranchLockfileNames = await getGitBranchLockfileNames(lockfileDir)
  expect(gitBranchLockfileNames).toEqual(['pnpm-lock.branch.yaml'])
})
