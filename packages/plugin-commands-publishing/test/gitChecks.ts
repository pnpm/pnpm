import { promises as fs } from 'fs'
import { REGISTRY_MOCK_PORT } from '@pnpm/registry-mock'
import prepare from '@pnpm/prepare'
import PnpmError from '@pnpm/error'
import git from 'graceful-git'
import tempy from 'tempy'

// eslint-disable-next-line
import * as enquirer from 'enquirer'

// eslint-disable-next-line
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

  await git.noRetry(['init'])
  await git.noRetry(['checkout', '-b', 'test'])
  await git.noRetry(['config', 'user.email', 'x@y.z'])
  await git.noRetry(['config', 'user.name', 'xyz'])
  await git.noRetry(['add', '*'])
  await git.noRetry(['commit', '-m', 'init', '--no-gpg-sign'])

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

  await git.noRetry(['init'])
  await git.noRetry(['checkout', '-b', 'master'])
  await git.noRetry(['config', 'user.email', 'x@y.z'])
  await git.noRetry(['config', 'user.name', 'xyz'])
  await git.noRetry(['add', '*'])
  await git.noRetry(['commit', '-m', 'init', '--no-gpg-sign'])

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

  await git.noRetry(['init'])
  await git.noRetry(['config', 'user.email', 'x@y.z'])
  await git.noRetry(['config', 'user.name', 'xyz'])
  await git.noRetry(['add', '*'])
  await git.noRetry(['commit', '-m', 'init', '--no-gpg-sign'])

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

  prepare({
    name: 'test-publish-package.json',
    version: '0.0.0',
  })

  await git.noRetry(['init'])
  await git.noRetry(['config', 'user.email', 'x@y.z'])
  await git.noRetry(['config', 'user.name', 'xyz'])
  await git.noRetry(['init', '--bare'], { cwd: remote })
  await git.noRetry(['add', '*'])
  await git.noRetry(['commit', '-m', 'init', '--no-gpg-sign'])
  await git.noRetry(['commit', '--allow-empty', '--allow-empty-message', '-m', '', '--no-gpg-sign'])
  await git.noRetry(['remote', 'add', 'origin', remote])
  await git.noRetry(['push', '-u', 'origin', 'master'])
  await git.noRetry(['reset', '--hard', 'HEAD~1'])

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
