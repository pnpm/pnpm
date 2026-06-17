import fs from 'node:fs'
import path from 'node:path'

import { expect, test } from '@jest/globals'
import { getCurrentBranch, isGitRepo, isWorkingTreeClean } from '@pnpm/network.git-utils'
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

test('getCurrentBranch reads branch from .git/HEAD without spawning git', async () => {
  const tempDir = temporaryDirectory()

  await execa('git', ['init'], { cwd: tempDir })
  await execa('git', ['checkout', '-b', 'bar'], { cwd: tempDir })

  await expect(getCurrentBranch({ cwd: tempDir })).resolves.toBe('bar')
})

test('getCurrentBranch returns null for detached HEAD', async () => {
  const tempDir = temporaryDirectory()

  await execa('git', ['init'], { cwd: tempDir })
  await execa('git', ['checkout', '-b', 'main'], { cwd: tempDir })
  await execa('git', ['config', 'user.email', 'test@test.com'], { cwd: tempDir })
  await execa('git', ['config', 'user.name', 'test'], { cwd: tempDir })
  await execa('git', ['config', 'commit.gpgsign', 'false'], { cwd: tempDir })
  await execa('git', ['commit', '--allow-empty', '-m', 'init'], { cwd: tempDir })
  await execa('git', ['checkout', '--detach', 'HEAD'], { cwd: tempDir })

  await expect(getCurrentBranch({ cwd: tempDir })).resolves.toBeNull()
})

test('getCurrentBranch returns null outside a git repo', async () => {
  const tempDir = temporaryDirectory()

  await expect(getCurrentBranch({ cwd: tempDir })).resolves.toBeNull()
})

test('isWorkingTreeClean', async () => {
  const tempDir = temporaryDirectory()
  process.chdir(tempDir)

  await execa('git', ['init'])

  await expect(isWorkingTreeClean()).resolves.toBe(true)

  fs.writeFileSync(path.join(tempDir, 'foo'), 'foo')

  await expect(isWorkingTreeClean()).resolves.toBe(false)
})
