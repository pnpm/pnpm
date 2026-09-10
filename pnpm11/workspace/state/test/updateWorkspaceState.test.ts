import fs from 'node:fs'
import path from 'node:path'

import { afterEach, beforeEach, expect, jest, test } from '@jest/globals'
import { logger } from '@pnpm/logger'
import { prepareEmpty } from '@pnpm/prepare'

import { getFilePath } from '../src/filePath.js'
import { updateWorkspaceState, type UpdateWorkspaceStateOptions, updateWorkspaceStateOrWarn } from '../src/index.js'

const originalLoggerWarn = logger.warn
beforeEach(() => {
  logger.warn = jest.fn(originalLoggerWarn)
})
afterEach(() => {
  logger.warn = originalLoggerWarn
})

function options (workspaceDir: string): UpdateWorkspaceStateOptions {
  return {
    allProjects: [],
    settings: {
      excludeLinksFromLockfile: false,
      linkWorkspacePackages: false,
      preferWorkspacePackages: false,
    },
    workspaceDir,
    pnpmfiles: [],
    filteredInstall: false,
  }
}

function cacheDirEntries (workspaceDir: string): string[] {
  return fs.readdirSync(path.dirname(getFilePath(workspaceDir))).sort()
}

test('updateWorkspaceState() leaves no temp file beside the state file', async () => {
  prepareEmpty()
  const workspaceDir = process.cwd()

  await updateWorkspaceState(options(workspaceDir))
  await updateWorkspaceState(options(workspaceDir))

  expect(fs.existsSync(getFilePath(workspaceDir))).toBe(true)
  expect(cacheDirEntries(workspaceDir)).toStrictEqual([path.basename(getFilePath(workspaceDir))])
})

// A rename onto a directory fails outright on Unix, which the retry
// classifier leaves alone. On Windows the same setup reports EPERM, which
// *is* retried, so this stays Unix-only rather than burning the retry budget.
const testOnUnix = process.platform === 'win32' ? test.skip : test

testOnUnix('updateWorkspaceState() rejects and leaves no temp file when the write cannot land', async () => {
  prepareEmpty()
  const workspaceDir = process.cwd()
  const cacheFile = getFilePath(workspaceDir)
  fs.mkdirSync(cacheFile, { recursive: true })

  await expect(updateWorkspaceState(options(workspaceDir))).rejects.toThrow()
  expect(cacheDirEntries(workspaceDir)).toStrictEqual([path.basename(cacheFile)])
})

testOnUnix('updateWorkspaceStateOrWarn() warns instead of failing the command', async () => {
  prepareEmpty()
  const workspaceDir = process.cwd()
  fs.mkdirSync(getFilePath(workspaceDir), { recursive: true })

  await expect(updateWorkspaceStateOrWarn(options(workspaceDir))).resolves.toBeUndefined()
  expect(jest.mocked(logger.warn).mock.calls).toHaveLength(1)
  expect(jest.mocked(logger.warn).mock.calls[0][0]).toMatchObject({
    prefix: workspaceDir,
  })
  expect((jest.mocked(logger.warn).mock.calls[0][0] as { message: string }).message)
    .toContain('Failed to update the workspace state')
})
