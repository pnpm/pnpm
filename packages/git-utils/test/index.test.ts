import fs from 'node:fs'
import path from 'node:path'

import { getCurrentBranch, isGitRepo, isWorkingTreeClean } from '@pnpm/git-utils'
import { safeExeca as execa } from 'execa'
import { temporaryDirectory } from 'tempy'

test('isGitRepo', async () => {
  const tempDir = temporaryDirectory()
  process.chdir(tempDir)

  await expect(isGitRepo()).resolves.toBe(false)

  await execa('git', ['init'])

  await expect(isGitRepo()).resolves.toBe(true)
})

test('getCurrentBranch', async () => {
  const tempDir = temporaryDirectory()
  process.chdir(tempDir)

  await execa('git', ['init'])
  await execa('git', ['checkout', '-b', 'foo'])

  await expect(getCurrentBranch()).resolves.toBe('foo')
})

test('isWorkingTreeClean', async () => {
  const tempDir = temporaryDirectory()
  process.chdir(tempDir)

  await execa('git', ['init'])

  await expect(isWorkingTreeClean()).resolves.toBe(true)

  fs.writeFileSync(path.join(tempDir, 'foo'), 'foo')

  await expect(isWorkingTreeClean()).resolves.toBe(false)
})
