import fs from 'node:fs'

import { expect, jest, test } from '@jest/globals'
import { PnpmError } from '@pnpm/error'
import { prepare } from '@pnpm/prepare'
import { safeExeca as execa } from 'execa'
import { temporaryDirectory } from 'tempy'

import { DEFAULT_OPTS } from './utils/index.js'

jest.unstable_mockModule('@inquirer/prompts', () => {
  class Separator {
    separator: string
    readonly type = 'separator' as const
    constructor (separator: string) {
      this.separator = separator
    }
  }
  return {
    Separator,
    checkbox: jest.fn(),
    confirm: jest.fn(),
    input: jest.fn(),
    password: jest.fn(),
    select: jest.fn(),
  }
})
const { confirm } = await import('@inquirer/prompts')
const { publish } = await import('@pnpm/releasing.commands')

const mockConfirm = jest.mocked(confirm)

test.each([false, true])('publish: fails git check if branch is not on master or main (CI=%s)', async (isCI) => {
  prepare({
    name: 'test-publish-package.json',
    version: '0.0.0',
  })

  await execa('git', ['init', '--initial-branch=test'])
  await execa('git', ['config', 'user.email', 'x@y.z'])
  await execa('git', ['config', 'user.name', 'xyz'])
  await execa('git', ['add', '*'])
  await execa('git', ['commit', '-m', 'init', '--no-gpg-sign'])

  mockConfirm.mockResolvedValue(false)

  await expect(
    publish.handler({
      ...DEFAULT_OPTS,
      ci: isCI,
      argv: { original: ['publish'] },
      dir: process.cwd(),
    }, [])
  ).rejects.toThrow(
    new PnpmError('GIT_NOT_CORRECT_BRANCH', "Branch is not on 'master|main'.")
  )
})

test.each([false, true])('publish: fails git check if branch is not on specified branch (CI=%s)', async (isCI) => {
  prepare({
    name: 'test-publish-package.json',
    version: '0.0.0',
  })

  await execa('git', ['init'])
  await execa('git', ['checkout', '-b', 'master'])
  await execa('git', ['config', 'user.email', 'x@y.z'])
  await execa('git', ['config', 'user.name', 'xyz'])
  await execa('git', ['add', '*'])
  await execa('git', ['commit', '-m', 'init', '--no-gpg-sign'])

  mockConfirm.mockResolvedValue(false)

  await expect(
    publish.handler({
      ...DEFAULT_OPTS,
      ci: isCI,
      argv: { original: ['publish'] },
      dir: process.cwd(),
      publishBranch: 'latest',
    }, [])
  ).rejects.toThrow(
    new PnpmError('GIT_NOT_CORRECT_BRANCH', "Branch is not on 'latest'.")
  )
})

test.each([false, true])('publish: fails git check if branch is not clean (CI=%s)', async (isCI) => {
  prepare({
    name: 'test-publish-package.json',
    version: '0.0.0',
  })

  await execa('git', ['init', '--initial-branch=main'])
  await execa('git', ['config', 'user.email', 'x@y.z'])
  await execa('git', ['config', 'user.name', 'xyz'])
  await execa('git', ['add', '*'])
  await execa('git', ['commit', '-m', 'init', '--no-gpg-sign'])

  fs.writeFileSync('LICENSE', 'workspace license', 'utf8')

  await expect(
    publish.handler({
      ...DEFAULT_OPTS,
      ci: isCI,
      argv: { original: ['publish'] },
      dir: process.cwd(),
    }, [])
  ).rejects.toThrow(
    new PnpmError('GIT_UNCLEAN', 'Unclean working tree. Commit or stash changes first.')
  )
})

test.each([false, true])('publish: fails git check if branch is not up to date (CI=%s)', async (isCI) => {
  const remote = temporaryDirectory()

  prepare({
    name: 'test-publish-package.json',
    version: '0.0.0',
  })

  await execa('git', ['init', '--initial-branch=main'])
  await execa('git', ['config', 'user.email', 'x@y.z'])
  await execa('git', ['config', 'user.name', 'xyz'])
  await execa('git', ['init', '--bare'], { cwd: remote })
  await execa('git', ['add', '*'])
  await execa('git', ['commit', '-m', 'init', '--no-gpg-sign'])
  await execa('git', ['commit', '--allow-empty', '--allow-empty-message', '-m', '', '--no-gpg-sign'])
  await execa('git', ['remote', 'add', 'origin', remote])
  await execa('git', ['push', '-u', 'origin', 'main'])
  await execa('git', ['reset', '--hard', 'HEAD~1'])

  await expect(
    publish.handler({
      ...DEFAULT_OPTS,
      ci: isCI,
      argv: { original: ['publish'] },
      dir: process.cwd(),
    }, [])
  ).rejects.toThrow(
    new PnpmError('GIT_NOT_LATEST', 'Remote history differs. Please pull changes.')
  )
})

test('publish: fails git check if HEAD is detached', async () => {
  prepare({
    name: 'test-publish-package.json',
    version: '0.0.0',
  })

  await execa('git', ['init'])
  await execa('git', ['config', 'user.email', 'x@y.z'])
  await execa('git', ['config', 'user.name', 'xyz'])
  await execa('git', ['add', '*'])
  await execa('git', ['commit', '-m', 'init', '--no-gpg-sign'])
  await execa('git', ['commit', '--allow-empty', '--allow-empty-message', '-m', '', '--no-gpg-sign'])
  await execa('git', ['checkout', 'HEAD~1'])

  await expect(
    publish.handler({
      ...DEFAULT_OPTS,
      ci: false,
      argv: { original: ['publish'] },
      dir: process.cwd(),
    }, [])
  ).rejects.toThrow(
    new PnpmError('GIT_UNKNOWN_BRANCH', 'The Git HEAD may not attached to any branch, but your "publish-branch" is set to "master|main".')
  )
})

test.each([undefined, 'release'])('publish: allows a detached tag in CI (publishBranch=%s)', async (publishBranch) => {
  await prepareDetachedTag()
  mockConfirm.mockClear()

  const result = await publish.handler({
    ...DEFAULT_OPTS,
    ci: true,
    argv: { original: ['publish'] },
    dir: process.cwd(),
    dryRun: true,
    publishBranch,
    json: true,
  }, [])

  expect(JSON.parse(result!.output!)).toMatchObject({ name: 'test-publish-package.json', version: '0.0.0' })
  expect(mockConfirm).not.toHaveBeenCalled()
})

test('publish: rejects a dirty detached tag in CI', async () => {
  await prepareDetachedTag()
  fs.writeFileSync('LICENSE', 'workspace license', 'utf8')

  await expect(publish.handler({
    ...DEFAULT_OPTS,
    ci: true,
    argv: { original: ['publish'] },
    dir: process.cwd(),
    dryRun: true,
  }, [])).rejects.toThrow(new PnpmError('GIT_UNCLEAN', 'Unclean working tree. Commit or stash changes first.'))
})

test('publish: rejects an unknown symbolic HEAD in CI', async () => {
  await prepareDetachedTag()
  await execa('git', ['symbolic-ref', 'HEAD', 'refs/tags/v0.0.0'])

  await expect(publish.handler({
    ...DEFAULT_OPTS,
    ci: true,
    argv: { original: ['publish'] },
    dir: process.cwd(),
    dryRun: true,
  }, [])).rejects.toThrow(new PnpmError('GIT_UNKNOWN_BRANCH', 'The Git HEAD may not attached to any branch, but your "publish-branch" is set to "master|main".'))
})

async function prepareDetachedTag (): Promise<void> {
  prepare({ name: 'test-publish-package.json', version: '0.0.0' })
  await execa('git', ['init', '--initial-branch=main'])
  await execa('git', ['config', 'user.email', 'x@y.z'])
  await execa('git', ['config', 'user.name', 'xyz'])
  await execa('git', ['add', '*'])
  await execa('git', ['commit', '-m', 'init', '--no-gpg-sign'])
  await execa('git', ['tag', '-a', 'v0.0.0', '-m', 'release', '--no-sign'])
  await execa('git', ['checkout', 'v0.0.0'])
}
