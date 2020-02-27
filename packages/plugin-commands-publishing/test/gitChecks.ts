import PnpmError from '@pnpm/error'
import { publish } from '@pnpm/plugin-commands-publishing'
import prepare from '@pnpm/prepare'
import { REGISTRY_MOCK_PORT } from '@pnpm/registry-mock'
import execa = require('execa')
import fs = require('mz/fs')
import path = require('path')
import test = require('tape')
import tempy = require('tempy')
import { DEFAULT_OPTS } from './utils'

const CREDENTIALS = [
  `--registry=http://localhost:${REGISTRY_MOCK_PORT}/`,
  `--//localhost:${REGISTRY_MOCK_PORT}/:username=username`,
  `--//localhost:${REGISTRY_MOCK_PORT}/:_password=${Buffer.from('password').toString('base64')}`,
  `--//localhost:${REGISTRY_MOCK_PORT}/:email=foo@bar.net`,
]

test('publish: fails git check if branch is not on master', async (t) => {
  const tempDir = tempy.directory()

  prepare(t, {
    name: 'test-publish-package.json',
    version: '0.0.0',
  }, {
    tempDir,
  })

  await execa('git', ['init'], { cwd: tempDir })
  await execa('git', ['checkout', '-b', 'test'], { cwd: tempDir })

  let err!: PnpmError
  try {
    await publish.handler([], {
      ...DEFAULT_OPTS,
      argv: { original: ['publish', ...CREDENTIALS] },
      dir: process.cwd(),
    })
  } catch (_err) {
    err = _err
  }
  t.ok(err)
  t.equal(err.code, 'ERR_PNPM_GIT_CHECK_FAILED')
  t.equal(err.message, "Branch is not on 'master'.")

  t.end()
})

test('publish: fails git check if branch is not clean', async (t) => {
  const tempDir = tempy.directory()

  prepare(t, {
    name: 'test-publish-package.json',
    version: '0.0.0',
  }, {
    tempDir,
  })

  await execa('git', ['init'], { cwd: tempDir })
  await execa('git', ['add', '*'], { cwd: tempDir })
  await execa('git', ['commit', '-m', 'init', '--no-gpg-sign'], { cwd: tempDir })

  await fs.writeFile(path.join(tempDir, 'LICENSE'), 'workspace license', 'utf8')

  let err!: PnpmError
  try {
    await publish.handler([], {
      ...DEFAULT_OPTS,
      argv: { original: ['publish', ...CREDENTIALS] },
      dir: process.cwd(),
    })
  } catch (_err) {
    err = _err
  }
  t.ok(err)
  t.equal(err.code, 'ERR_PNPM_GIT_CHECK_FAILED')
  t.equal(err.message, 'Unclean working tree. Commit or stash changes first.')

  t.end()
})

test('publish: fails git check if branch is not update to date', async (t) => {
  const tempDir = tempy.directory()
  const remote = tempy.directory()

  prepare(t, {
    name: 'test-publish-package.json',
    version: '0.0.0',
  }, {
    tempDir,
  })

  await execa('git', ['init'], { cwd: tempDir })
  await execa('git', ['init', '--bare'], { cwd: remote })
  await execa('git', ['add', '*'], { cwd: tempDir })
  await execa('git', ['commit', '-m', 'init', '--no-gpg-sign'], { cwd: tempDir })
  await execa('git', ['commit', '--allow-empty', '--allow-empty-message', '-m', '', '--no-gpg-sign'], { cwd: tempDir })
  await execa('git', ['remote', 'add', 'origin', remote], { cwd: tempDir })
  await execa('git', ['push', '-u', 'origin', 'master'], { cwd: tempDir })
  await execa('git', ['reset', '--hard', 'HEAD~1'], { cwd: tempDir })

  let err!: PnpmError
  try {
    await publish.handler([], {
      ...DEFAULT_OPTS,
      argv: { original: ['publish', ...CREDENTIALS] },
      dir: process.cwd(),
    })
  } catch (_err) {
    err = _err
  }
  t.ok(err)
  t.equal(err.code, 'ERR_PNPM_GIT_CHECK_FAILED')
  t.equal(err.message, 'Remote history differs. Please pull changes.')

  t.end()
})
