import { WANTED_LOCKFILE } from '@pnpm/constants'
import { getWantedLockfileName } from '../lib/lockfileName'
import { getCurrentBranch } from '../../git-utils/test/utils/mock'

describe('lockfileName', () => {
  afterEach(() => {
    getCurrentBranch.mockReset()
  })

  test('returns default lockfile name if useGitBranchLockfile is off', async () => {
    await expect(getWantedLockfileName()).resolves.toBe(WANTED_LOCKFILE)
  })

  test('returns git branch lockfile name', async () => {
    getCurrentBranch.mockReturnValue('main')
    await expect(getWantedLockfileName({ useGitBranchLockfile: true })).resolves.toBe('pnpm-lock.main.yaml')
  })

  test('returns git branch lockfile name when git branch contains clashes', async () => {
    getCurrentBranch.mockReturnValue('a/b/c')
    await expect(getWantedLockfileName({ useGitBranchLockfile: true })).resolves.toBe('pnpm-lock.a!b!c.yaml')
  })

  test('returns git branch lockfile name when git branch contains uppercase', async () => {
    getCurrentBranch.mockReturnValue('aBc')
    await expect(getWantedLockfileName({ useGitBranchLockfile: true })).resolves.toBe('pnpm-lock.abc.yaml')
  })
})
