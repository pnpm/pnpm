import { WANTED_LOCKFILE } from '@pnpm/constants'
import { jest } from '@jest/globals'

jest.unstable_mockModule('@pnpm/git-utils', () => ({ getCurrentBranch: jest.fn() }))

const { getCurrentBranch } = await import('@pnpm/git-utils')
const { getWantedLockfileName } = await import('../lib/lockfileName.js')

describe('lockfileName', () => {
  afterEach(() => {
    jest.mocked(getCurrentBranch).mockReset()
  })

  test('returns default lockfile name if useGitBranchLockfile is off', async () => {
    await expect(getWantedLockfileName()).resolves.toBe(WANTED_LOCKFILE)
  })

  test('returns git branch lockfile name', async () => {
    jest.mocked(getCurrentBranch).mockReturnValue(Promise.resolve('main'))
    await expect(getWantedLockfileName({ useGitBranchLockfile: true })).resolves.toBe('pnpm-lock.main.yaml')
  })

  test('returns git branch lockfile name when git branch contains clashes', async () => {
    jest.mocked(getCurrentBranch).mockReturnValue(Promise.resolve('a/b/c'))
    await expect(getWantedLockfileName({ useGitBranchLockfile: true })).resolves.toBe('pnpm-lock.a!b!c.yaml')
  })

  test('returns git branch lockfile name when git branch contains uppercase', async () => {
    jest.mocked(getCurrentBranch).mockReturnValue(Promise.resolve('aBc'))
    await expect(getWantedLockfileName({ useGitBranchLockfile: true })).resolves.toBe('pnpm-lock.abc.yaml')
  })
})
