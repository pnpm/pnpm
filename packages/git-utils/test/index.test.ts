import path from 'node:path'
import { promises as fs } from 'node:fs'

import tempy from 'tempy'
import execa from 'execa'

import {
  isGitRepo,
  getCurrentBranch,
  isWorkingTreeClean,
} from '@pnpm/git-utils'

test('isGitRepo', async () => {
  const tempDir = tempy.directory()

  process.chdir(tempDir)

  await expect(isGitRepo()).resolves.toBe(false)

  await execa('git', ['init'])

  await expect(isGitRepo()).resolves.toBe(true)
})

test('getCurrentBranch', async () => {
  const tempDir = tempy.directory()

  process.chdir(tempDir)

  await execa('git', ['init'])

  await execa('git', ['checkout', '-b', 'foo'])

  await expect(getCurrentBranch()).resolves.toBe('foo')
})

test('isWorkingTreeClean', async () => {
  const tempDir = tempy.directory()

  process.chdir(tempDir)

  await execa('git', ['init'])

  await expect(isWorkingTreeClean()).resolves.toBe(true)

  await fs.writeFile(path.join(tempDir, 'foo'), 'foo')

  await expect(isWorkingTreeClean()).resolves.toBe(false)
})
