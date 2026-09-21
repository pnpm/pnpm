import fs from 'node:fs'
import path from 'node:path'

import { expect, test } from '@jest/globals'
import { getCurrentBranch, isGitRepo, isHeadDetached, isWorkingTreeClean, nonInteractiveGitEnv } from '@pnpm/network.git-utils'
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
  await expect(isHeadDetached()).resolves.toBe(false)
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
  await expect(isHeadDetached({ cwd: tempDir })).resolves.toBe(false)
  await execa('git', ['checkout', '--detach', 'HEAD'], { cwd: tempDir })

  await expect(getCurrentBranch({ cwd: tempDir })).resolves.toBeNull()
  await expect(isHeadDetached({ cwd: tempDir })).resolves.toBe(true)
  const subdir = path.join(tempDir, 'subdir')
  fs.mkdirSync(subdir)
  await expect(isHeadDetached({ cwd: subdir })).resolves.toBe(true)
})

test('getCurrentBranch returns null outside a git repo', async () => {
  const tempDir = temporaryDirectory()

  await expect(getCurrentBranch({ cwd: tempDir })).resolves.toBeNull()
  await expect(isHeadDetached({ cwd: tempDir })).resolves.toBe(false)
})

test('isWorkingTreeClean', async () => {
  const tempDir = temporaryDirectory()
  process.chdir(tempDir)

  await execa('git', ['init'])

  await expect(isWorkingTreeClean()).resolves.toBe(true)

  fs.writeFileSync(path.join(tempDir, 'foo'), 'foo')

  await expect(isWorkingTreeClean()).resolves.toBe(false)
})

test('nonInteractiveGitEnv disables git and ssh prompts', async () => {
  await withIsolatedGitConfig(async () => {
    await expect(nonInteractiveGitEnv()).resolves.toMatchObject({
      GIT_TERMINAL_PROMPT: '0',
      GIT_SSH_COMMAND: 'ssh -o BatchMode=yes',
    })
  })
})

test('nonInteractiveGitEnv keeps an ssh command selected through the environment', async () => {
  await withIsolatedGitConfig(async () => {
    process.env.GIT_SSH_COMMAND = 'ssh -i ~/.ssh/deploy_key'
    await expect(nonInteractiveGitEnv()).resolves.toMatchObject({
      GIT_SSH_COMMAND: 'ssh -i ~/.ssh/deploy_key',
      GIT_TERMINAL_PROMPT: '0',
    })

    delete process.env.GIT_SSH_COMMAND
    process.env.GIT_SSH = 'plink'
    const gitEnv = await nonInteractiveGitEnv()
    expect(gitEnv).toMatchObject({ GIT_SSH: 'plink', GIT_TERMINAL_PROMPT: '0' })
    expect(gitEnv).not.toHaveProperty('GIT_SSH_COMMAND')
  })
})

test('nonInteractiveGitEnv keeps an ssh command selected through git configuration', async () => {
  await withIsolatedGitConfig(async () => {
    await execa('git', ['init'])
    await execa('git', ['config', 'core.sshCommand', 'ssh -i ~/.ssh/deploy_key'])

    const gitEnv = await nonInteractiveGitEnv()
    expect(gitEnv).toMatchObject({ GIT_TERMINAL_PROMPT: '0' })
    expect(gitEnv).not.toHaveProperty('GIT_SSH_COMMAND')
  })
})

/**
 * Runs `fn` in a fresh directory where only the git configuration written by
 * the test is in effect, with the ssh selection variables unset.
 */
async function withIsolatedGitConfig (fn: () => Promise<void>): Promise<void> {
  const tempDir = temporaryDirectory()
  process.chdir(tempDir)
  const names = ['GIT_SSH', 'GIT_SSH_COMMAND', 'GIT_CONFIG_GLOBAL', 'GIT_CONFIG_NOSYSTEM']
  const original = Object.fromEntries(names.map((name) => [name, process.env[name]]))
  delete process.env.GIT_SSH
  delete process.env.GIT_SSH_COMMAND
  process.env.GIT_CONFIG_GLOBAL = path.join(tempDir, 'empty-gitconfig')
  process.env.GIT_CONFIG_NOSYSTEM = '1'
  fs.writeFileSync(process.env.GIT_CONFIG_GLOBAL, '')
  try {
    await fn()
  } finally {
    for (const [name, value] of Object.entries(original)) {
      if (value === undefined) {
        delete process.env[name]
      } else {
        process.env[name] = value
      }
    }
  }
}
