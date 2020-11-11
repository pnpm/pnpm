import PnpmError from '@pnpm/error'
import prepare from '@pnpm/prepare'
import { REGISTRY_MOCK_PORT } from '@pnpm/registry-mock'
import { DEFAULT_OPTS } from './utils'
import execa = require('execa')
import fs = require('mz/fs')
import tempy = require('tempy')

jest.mock('enquirer', () => ({ prompt: jest.fn() }))

// eslint-disable-next-line
import * as enquirer from 'enquirer'

// eslint-disable-next-line
const prompt = enquirer.prompt as any

// eslint-disable-next-line
import { publish } from '@pnpm/plugin-commands-publishing'

const CREDENTIALS = [
  `--registry=http://localhost:${REGISTRY_MOCK_PORT}/`,
  `--//localhost:${REGISTRY_MOCK_PORT}/:username=username`,
  `--//localhost:${REGISTRY_MOCK_PORT}/:_password=${Buffer.from('password').toString('base64')}`,
  `--//localhost:${REGISTRY_MOCK_PORT}/:email=foo@bar.net`,
]

test('publish: fails git check if branch is not on master', async () => {
  prepare(undefined, {
    name: 'test-publish-package.json',
    version: '0.0.0',
  })

  await execa('git', ['init'])
  await execa('git', ['checkout', '-b', 'test'])
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
    new PnpmError('GIT_NOT_CORRECT_BRANCH', "Branch is not on 'master'.")
  )
})

test('publish: fails git check if branch is not on specified branch', async () => {
  prepare(undefined, {
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
  prepare(undefined, {
    name: 'test-publish-package.json',
    version: '0.0.0',
  })

  await execa('git', ['init'])
  await execa('git', ['config', 'user.email', 'x@y.z'])
  await execa('git', ['config', 'user.name', 'xyz'])
  await execa('git', ['add', '*'])
  await execa('git', ['commit', '-m', 'init', '--no-gpg-sign'])

  await fs.writeFile('LICENSE', 'workspace license', 'utf8')

  await expect(
    publish.handler({
      ...DEFAULT_OPTS,
      argv: { original: ['publish', ...CREDENTIALS] },
      dir: process.cwd(),
    }, [])
  ).rejects.toThrow(
    new PnpmError('GIT_NOT_UNCLEAN', 'Unclean working tree. Commit or stash changes first.')
  )
})

test('publish: fails git check if branch is not up-to-date', async () => {
  const remote = tempy.directory()

  prepare(undefined, {
    name: 'test-publish-package.json',
    version: '0.0.0',
  })

  await execa('git', ['init'])
  await execa('git', ['config', 'user.email', 'x@y.z'])
  await execa('git', ['config', 'user.name', 'xyz'])
  await execa('git', ['init', '--bare'], { cwd: remote })
  await execa('git', ['add', '*'])
  await execa('git', ['commit', '-m', 'init', '--no-gpg-sign'])
  await execa('git', ['commit', '--allow-empty', '--allow-empty-message', '-m', '', '--no-gpg-sign'])
  await execa('git', ['remote', 'add', 'origin', remote])
  await execa('git', ['push', '-u', 'origin', 'master'])
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
