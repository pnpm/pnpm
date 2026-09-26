import { afterEach, describe, expect, jest, test } from '@jest/globals'
import { WANTED_LOCKFILE } from '@pnpm/constants'

jest.unstable_mockModule('@pnpm/network.git-utils', () => ({
  getCurrentBranch: jest.fn(),
  getBranchesContainingHead: jest.fn(() => Promise.resolve([])),
}))

const { getCurrentBranch, getBranchesContainingHead } = await import('@pnpm/network.git-utils')
const { getWantedLockfileName, selectWantedLockfile } = await import('../lib/lockfileName.js')

describe('lockfileName', () => {
  afterEach(() => {
    jest.mocked(getCurrentBranch).mockReset()
    jest.mocked(getBranchesContainingHead).mockReset()
    jest.mocked(getBranchesContainingHead).mockReturnValue(Promise.resolve([]))
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

  test('replaces each UTF-16 code unit of a character outside the Basic Multilingual Plane', async () => {
    jest.mocked(getCurrentBranch).mockReturnValue(Promise.resolve('feat/🚀x'))
    await expect(getWantedLockfileName({ useGitBranchLockfile: true })).resolves.toBe('pnpm-lock.feat!!!x.yaml')
  })

  test('passes cwd to getCurrentBranch', async () => {
    jest.mocked(getCurrentBranch).mockReturnValue(Promise.resolve('main'))
    await getWantedLockfileName({ useGitBranchLockfile: true, cwd: '/some/workspace' })
    expect(jest.mocked(getCurrentBranch)).toHaveBeenCalledWith({ cwd: '/some/workspace' })
  })

  test('selectWantedLockfile names the shared lockfile when useGitBranchLockfile is off', async () => {
    jest.mocked(getCurrentBranch).mockReturnValue(Promise.resolve('main'))
    await expect(selectWantedLockfile()).resolves.toStrictEqual({
      fileName: WANTED_LOCKFILE,
      detachedHeadCandidates: [],
    })
  })

  test('selectWantedLockfile names the shared lockfile under mergeGitBranchLockfiles', async () => {
    jest.mocked(getCurrentBranch).mockReturnValue(Promise.resolve('main'))
    await expect(
      selectWantedLockfile({ useGitBranchLockfile: true, mergeGitBranchLockfiles: true })
    ).resolves.toStrictEqual({ fileName: WANTED_LOCKFILE, detachedHeadCandidates: [] })
  })

  test('selectWantedLockfile names the current branch lockfile', async () => {
    jest.mocked(getCurrentBranch).mockReturnValue(Promise.resolve('feature/Login'))
    await expect(selectWantedLockfile({ useGitBranchLockfile: true })).resolves.toStrictEqual({
      fileName: 'pnpm-lock.feature!login.yaml',
      detachedHeadCandidates: [],
    })
  })

  test('selectWantedLockfile lists the lockfiles of the branches containing a detached HEAD', async () => {
    jest.mocked(getCurrentBranch).mockReturnValue(Promise.resolve(null))
    jest.mocked(getBranchesContainingHead).mockReturnValue(Promise.resolve(['feature', 'main']))
    await expect(selectWantedLockfile({ useGitBranchLockfile: true })).resolves.toStrictEqual({
      fileName: WANTED_LOCKFILE,
      detachedHeadCandidates: ['pnpm-lock.feature.yaml', 'pnpm-lock.main.yaml'],
    })
  })

  test('selectWantedLockfile skips a detached HEAD candidate too long to be a file name', async () => {
    jest.mocked(getCurrentBranch).mockReturnValue(Promise.resolve(null))
    jest.mocked(getBranchesContainingHead).mockReturnValue(Promise.resolve(['a'.repeat(250), 'main']))
    await expect(selectWantedLockfile({ useGitBranchLockfile: true })).resolves.toStrictEqual({
      fileName: WANTED_LOCKFILE,
      detachedHeadCandidates: ['pnpm-lock.main.yaml'],
    })
  })

  test('selectWantedLockfile has no candidates when nothing contains the detached HEAD', async () => {
    jest.mocked(getCurrentBranch).mockReturnValue(Promise.resolve(null))
    jest.mocked(getBranchesContainingHead).mockReturnValue(Promise.resolve([]))
    await expect(selectWantedLockfile({ useGitBranchLockfile: true })).resolves.toStrictEqual({
      fileName: WANTED_LOCKFILE,
      detachedHeadCandidates: [],
    })
  })
})
