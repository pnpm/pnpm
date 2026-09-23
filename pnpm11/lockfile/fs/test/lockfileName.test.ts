import fs from 'node:fs'
import path from 'node:path'

import { afterEach, describe, expect, jest, test } from '@jest/globals'
import { WANTED_LOCKFILE } from '@pnpm/constants'
import { temporaryDirectory } from 'tempy'

jest.unstable_mockModule('@pnpm/network.git-utils', () => ({
  getBranchCandidatesFromGit: jest.fn(async () => []),
  getBranchFromCiEnv: jest.fn(),
  getCurrentBranch: jest.fn(),
}))

const { getBranchCandidatesFromGit, getBranchFromCiEnv, getCurrentBranch } = await import('@pnpm/network.git-utils')
const { getWantedLockfileName } = await import('../lib/lockfileName.js')

describe('lockfileName', () => {
  afterEach(() => {
    jest.mocked(getCurrentBranch).mockReset()
    jest.mocked(getBranchFromCiEnv).mockReset()
    jest.mocked(getBranchCandidatesFromGit).mockReset()
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

  test('passes cwd to getCurrentBranch', async () => {
    jest.mocked(getCurrentBranch).mockReturnValue(Promise.resolve('main'))
    await getWantedLockfileName({ useGitBranchLockfile: true, cwd: '/some/workspace' })
    expect(jest.mocked(getCurrentBranch)).toHaveBeenCalledWith({ cwd: '/some/workspace' })
  })

  test('uses CI branch when getCurrentBranch returns null', async () => {
    jest.mocked(getCurrentBranch).mockReturnValue(Promise.resolve(null))
    jest.mocked(getBranchFromCiEnv).mockReturnValue('ci-feature')
    await expect(getWantedLockfileName({ useGitBranchLockfile: true })).resolves.toBe('pnpm-lock.ci-feature.yaml')
    expect(jest.mocked(getBranchFromCiEnv)).toHaveBeenCalled()
  })

  test('matches candidate branch from git on detached HEAD when branch lockfile exists', async () => {
    jest.mocked(getCurrentBranch).mockReturnValue(Promise.resolve(null))
    jest.mocked(getBranchFromCiEnv).mockReturnValue(null)
    jest.mocked(getBranchCandidatesFromGit).mockReturnValue(Promise.resolve(['feature-x']))
    const tempDir = temporaryDirectory()
    fs.writeFileSync(path.join(tempDir, 'pnpm-lock.feature-x.yaml'), '')
    await expect(getWantedLockfileName({ useGitBranchLockfile: true, cwd: tempDir })).resolves.toBe('pnpm-lock.feature-x.yaml')
  })

  test('falls back to default lockfile name on detached HEAD when no candidate matches', async () => {
    jest.mocked(getCurrentBranch).mockReturnValue(Promise.resolve(null))
    jest.mocked(getBranchFromCiEnv).mockReturnValue(null)
    jest.mocked(getBranchCandidatesFromGit).mockReturnValue(Promise.resolve(['feature-y']))
    const tempDir = temporaryDirectory()
    fs.writeFileSync(path.join(tempDir, 'pnpm-lock.other.yaml'), '')
    await expect(getWantedLockfileName({ useGitBranchLockfile: true, cwd: tempDir })).resolves.toBe(WANTED_LOCKFILE)
  })

  test('uses lockfileDir for locating branch lockfiles when distinct from cwd', async () => {
    jest.mocked(getCurrentBranch).mockReturnValue(Promise.resolve(null))
    jest.mocked(getBranchFromCiEnv).mockReturnValue(null)
    jest.mocked(getBranchCandidatesFromGit).mockReturnValue(Promise.resolve(['feature-x']))
    const lockfileDir = temporaryDirectory()
    const gitCwd = temporaryDirectory()
    fs.writeFileSync(path.join(lockfileDir, 'pnpm-lock.feature-x.yaml'), '')
    await expect(getWantedLockfileName({
      useGitBranchLockfile: true,
      cwd: gitCwd,
      lockfileDir,
    })).resolves.toBe('pnpm-lock.feature-x.yaml')
    expect(jest.mocked(getBranchCandidatesFromGit)).toHaveBeenCalledWith({ cwd: gitCwd })
  })

  test('falls back to default lockfile name when lockfileDir does not exist (ENOENT)', async () => {
    jest.mocked(getCurrentBranch).mockReturnValue(Promise.resolve(null))
    jest.mocked(getBranchFromCiEnv).mockReturnValue(null)
    const nonExistentDir = path.join(temporaryDirectory(), 'absent')
    await expect(getWantedLockfileName({
      useGitBranchLockfile: true,
      cwd: nonExistentDir,
    })).resolves.toBe(WANTED_LOCKFILE)
  })

  test('rethrows unexpected errors from inspecting lockfileDir', async () => {
    jest.mocked(getCurrentBranch).mockReturnValue(Promise.resolve(null))
    jest.mocked(getBranchFromCiEnv).mockReturnValue(null)
    const tempFile = path.join(temporaryDirectory(), 'file-not-a-dir')
    fs.writeFileSync(tempFile, 'hello')
    // Calling readdir on a regular file yields ENOTDIR, which should be rethrown
    await expect(getWantedLockfileName({
      useGitBranchLockfile: true,
      cwd: tempFile,
    })).rejects.toThrow()
  })
})
