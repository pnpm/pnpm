import fs from 'node:fs'
import path from 'node:path'

import { expect, test } from '@jest/globals'
import { prepareEmpty } from '@pnpm/prepare'

import { getFilePath } from '../src/filePath.js'
import { updateWorkspaceState, type UpdateWorkspaceStateOptions } from '../src/index.js'

test('updateWorkspaceState() leaves no temp file beside the state file', async () => {
  prepareEmpty()
  const workspaceDir = process.cwd()

  await updateWorkspaceState(options(workspaceDir))
  await updateWorkspaceState(options(workspaceDir))

  expect(fs.existsSync(getFilePath(workspaceDir))).toBe(true)
  expect(cacheDirEntries(workspaceDir)).toStrictEqual([path.basename(getFilePath(workspaceDir))])
})

// A rename onto a directory fails outright on Unix. On Windows the same setup
// reports EPERM, which is retried, so the test would spend the retry budget.
const testOnUnix = process.platform === 'win32' ? test.skip : test

testOnUnix('updateWorkspaceState() leaves no temp file when the write cannot land', async () => {
  prepareEmpty()
  const workspaceDir = process.cwd()
  const cacheFile = getFilePath(workspaceDir)
  fs.mkdirSync(cacheFile, { recursive: true })

  await expect(updateWorkspaceState(options(workspaceDir))).resolves.toBeUndefined()
  expect(cacheDirEntries(workspaceDir)).toStrictEqual([path.basename(cacheFile)])
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
