import fs from 'fs'
import { REGISTRY_MOCK_PORT } from '@pnpm/registry-mock'
import { prepare } from '@pnpm/prepare'
import { PnpmError } from '@pnpm/error'
import execa from 'execa'
import tempy from 'tempy'

import * as enquirer from 'enquirer'

import { publish } from '@pnpm/plugin-commands-publishing'
import { DEFAULT_OPTS } from './utils'

jest.mock('enquirer', () => ({ prompt: jest.fn() }))

// eslint-disable-next-line
const prompt = enquirer.prompt as any

const CREDENTIALS = [
  `--registry=http://localhost:${REGISTRY_MOCK_PORT}/`,
  `--//localhost:${REGISTRY_MOCK_PORT}/:username=username`,
  `--//localhost:${REGISTRY_MOCK_PORT}/:_password=${Buffer.from('password').toString('base64')}`,
  `--//localhost:${REGISTRY_MOCK_PORT}/:email=foo@bar.net`,
]

test('publish: fails git check if branch is not on master or main', async () => {
  prepare({
    name: 'test-publish-package.json',
    version: '0.0.0',
  })

  await execa('git', ['init', '--initial-branch=test'])
  await execa('git', ['config', 'user.email', 'x@y.z'])
  await execa('git', ['config', 'user.name', 'xyz'])
  await execa('git', ['add', '*'])
  await execa('git', ['commit', '-m', 'init', '--no-gpg-sign'])

  prompt.mockResolvedValue({
    confirm: false,
  })

  await expect(
    publish.handler({
      ...DEFAULT_OPTS,
      argv: { original: ['publish', ...CREDENTIALS] },
      dir: process.cwd(),
    }, [])
  ).rejects.toThrow(
    new PnpmError('GIT_NOT_CORRECT_BRANCH', "Branch is not on 'master|main'.")
  )
})

test('publish: fails git check if branch is not on specified branch', async () => {
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

  prompt.mockResolvedValue({
    confirm: false,
  })

  await expect(
    publish.handler({
      ...DEFAULT_OPTS,
      argv: { original: ['publish', ...CREDENTIALS] },
      dir: process.cwd(),
      publishBranch: 'latest',
    }, [])
  ).rejects.toThrow(
    new PnpmError('GIT_NOT_CORRECT_BRANCH', "Branch is not on 'latest'.")
  )
})

test('publish: fails git check if branch is not clean', async () => {
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
      argv: { original: ['publish', ...CREDENTIALS] },
      dir: process.cwd(),
    }, [])
  ).rejects.toThrow(
    new PnpmError('GIT_UNCLEAN', 'Unclean working tree. Commit or stash changes first.')
  )
})

test('publish: fails git check if branch is not up to date', async () => {
  const remote = tempy.directory()

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
      argv: { original: ['publish', ...CREDENTIALS] },
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
      argv: { original: ['publish', ...CREDENTIALS] },
      dir: process.cwd(),
    }, [])
  ).rejects.toThrow(
    new PnpmError('GIT_UNKNOWN_BRANCH', 'The Git HEAD may not attached to any branch, but your "publish-branch" is set to "master|main".')
  )
})
