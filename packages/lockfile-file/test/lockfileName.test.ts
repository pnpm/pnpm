/* eslint-disable import/first */
const getCurrentBranchName = jest.fn()
jest.mock('@pnpm/lockfile-file/lib/gitChecks', () => {
  const original = jest.requireActual('@pnpm/lockfile-file/lib/gitChecks')
  return {
    ...original,
    getCurrentBranchName,
  }
})

import { WANTED_LOCKFILE } from '@pnpm/constants'
import { getWantedLockfileName } from '@pnpm/lockfile-file/lib/lockfileName'

describe('lockfileName', () => {
  afterEach(() => {
    getCurrentBranchName.mockReset()
  })

  test('returns default lockfile name if useGitBranchLockfile is off', () => {
    expect(getWantedLockfileName()).toBe(WANTED_LOCKFILE)
  })

  test('returns git branch lockfile name', () => {
    getCurrentBranchName.mockReturnValue('main')
    expect(getWantedLockfileName({ useGitBranchLockfile: true })).toBe('pnpm-lock.main.yaml')
  })

  test('returns git branch lockfile name when git branch contains clashes', () => {
    getCurrentBranchName.mockReturnValue('a/b/c')
    expect(getWantedLockfileName({ useGitBranchLockfile: true })).toBe('pnpm-lock.a!b!c.yaml')
  })
})
