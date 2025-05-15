import { WANTED_LOCKFILE } from '@pnpm/constants'
import { getCurrentBranch } from '@pnpm/git-utils'
import { getWantedLockfileName } from '../lib/lockfileName'

jest.mock('@pnpm/git-utils', () => ({ getCurrentBranch: jest.fn() }))

describe('lockfileName', () => {
  afterEach(() => {
    (getCurrentBranch as jest.Mock).mockReset()
  })

  test('returns default lockfile name if useGitBranchLockfile is off', async () => {
    await expect(getWantedLockfileName()).resolves.toBe(WANTED_LOCKFILE)
  })

  test('returns git branch lockfile name', async () => {
    (getCurrentBranch as jest.Mock).mockReturnValue('main')
    await expect(getWantedLockfileName({ useGitBranchLockfile: true })).resolves.toBe('pnpm-lock.main.yaml')
  })

  test('returns git branch lockfile name when git branch contains clashes', async () => {
    (getCurrentBranch as jest.Mock).mockReturnValue('a/b/c')
    await expect(getWantedLockfileName({ useGitBranchLockfile: true })).resolves.toBe('pnpm-lock.a!b!c.yaml')
  })

  test('returns git branch lockfile name when git branch contains uppercase', async () => {
    (getCurrentBranch as jest.Mock).mockReturnValue('aBc')
    await expect(getWantedLockfileName({ useGitBranchLockfile: true })).resolves.toBe('pnpm-lock.abc.yaml')
  })
})
