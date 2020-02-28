import PnpmError from '@pnpm/error'
import prepare from '@pnpm/prepare'
import { REGISTRY_MOCK_PORT } from '@pnpm/registry-mock'
import execa = require('execa')
import fs = require('mz/fs')
import proxyquire = require('proxyquire')
import sinon = require('sinon')
import test = require('tape')
import tempy = require('tempy')
import { DEFAULT_OPTS } from './utils'

const prompt = sinon.stub()

const publish = proxyquire('../lib/publish', {
  'enquirer': { prompt },
})

const CREDENTIALS = [
  `--registry=http://localhost:${REGISTRY_MOCK_PORT}/`,
  `--//localhost:${REGISTRY_MOCK_PORT}/:username=username`,
  `--//localhost:${REGISTRY_MOCK_PORT}/:_password=${Buffer.from('password').toString('base64')}`,
  `--//localhost:${REGISTRY_MOCK_PORT}/:email=foo@bar.net`,
]

test('publish: fails git check if branch is not on master', async (t) => {
  prepare(t, {
    name: 'test-publish-package.json',
    version: '0.0.0',
  })

  await execa('git', ['init'])
  await execa('git', ['checkout', '-b', 'test'])

  prompt.returns({
    confirm: false,
  })

  let err!: PnpmError
  try {
    await publish.handler([], {
      ...DEFAULT_OPTS,
      argv: { original: ['publish', ...CREDENTIALS] },
      dir: process.cwd(),
      gitChecks: true,
    })
  } catch (_err) {
    err = _err
  }
  t.ok(err)
  t.equal(err.code, 'ERR_PNPM_GIT_NOT_CORRECT_BRANCH')
  t.equal(err.message, "Branch is not on 'master'.")

  t.end()
})

test('publish: fails git check if branch is not on specified branch', async (t) => {
  prepare(t, {
    name: 'test-publish-package.json',
    version: '0.0.0',
  })

  await execa('git', ['init'])
  await execa('git', ['checkout', '-b', 'master'])

  prompt.returns({
    confirm: false,
  })

  let err!: PnpmError
  try {
    await publish.handler([], {
      ...DEFAULT_OPTS,
      argv: { original: ['publish', ...CREDENTIALS] },
      dir: process.cwd(),
      gitChecks: true,
      publishBranch: 'latest',
    })
  } catch (_err) {
    err = _err
  }
  t.ok(err)
  t.equal(err.code, 'ERR_PNPM_GIT_NOT_CORRECT_BRANCH')
  t.equal(err.message, "Branch is not on 'latest'.")

  t.end()
})

test('publish: fails git check if branch is not clean', async (t) => {
  prepare(t, {
    name: 'test-publish-package.json',
    version: '0.0.0',
  })

  await execa('git', ['init'])
  await execa('git', ['add', '*'])
  await execa('git', ['commit', '-m', 'init', '--no-gpg-sign'])

  await fs.writeFile('LICENSE', 'workspace license', 'utf8')

  let err!: PnpmError
  try {
    await publish.handler([], {
      ...DEFAULT_OPTS,
      argv: { original: ['publish', ...CREDENTIALS] },
      dir: process.cwd(),
      gitChecks: true,
    })
  } catch (_err) {
    err = _err
  }
  t.ok(err)
  t.equal(err.code, 'ERR_PNPM_GIT_NOT_UNCLEAN')
  t.equal(err.message, 'Unclean working tree. Commit or stash changes first.')

  t.end()
})

test('publish: fails git check if branch is not update to date', async (t) => {
  const remote = tempy.directory()

  prepare(t, {
    name: 'test-publish-package.json',
    version: '0.0.0',
  })

  await execa('git', ['init'])
  await execa('git', ['init', '--bare'], { cwd: remote })
  await execa('git', ['add', '*'])
  await execa('git', ['commit', '-m', 'init', '--no-gpg-sign'])
  await execa('git', ['commit', '--allow-empty', '--allow-empty-message', '-m', '', '--no-gpg-sign'])
  await execa('git', ['remote', 'add', 'origin', remote])
  await execa('git', ['push', '-u', 'origin', 'master'])
  await execa('git', ['reset', '--hard', 'HEAD~1'])

  let err!: PnpmError
  try {
    await publish.handler([], {
      ...DEFAULT_OPTS,
      argv: { original: ['publish', ...CREDENTIALS] },
      dir: process.cwd(),
      gitChecks: true,
    })
  } catch (_err) {
    err = _err
  }
  t.ok(err)
  t.equal(err.code, 'ERR_PNPM_GIT_NOT_LATEST')
  t.equal(err.message, 'Remote history differs. Please pull changes.')

  t.end()
})
