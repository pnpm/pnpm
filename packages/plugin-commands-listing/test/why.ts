import PnpmError from '@pnpm/error'
import { why } from '@pnpm/plugin-commands-listing'
import prepare from '@pnpm/prepare'
import { REGISTRY_MOCK_PORT } from '@pnpm/registry-mock'
import execa = require('execa')
import stripAnsi = require('strip-ansi')
import test = require('tape')

test('`pnpm why` should fail if no package name was provided', async (t) => {
  prepare(t)

  let err!: PnpmError
  try {
    await why.handler({
      dir: process.cwd(),
    }, [])
  } catch (_err) {
    err = _err
  }

  t.equal(err.code, 'ERR_PNPM_MISSING_PACKAGE_NAME')
  t.ok(err.message.includes('`pnpm why` requires the package name'))
  t.end()
})

test('"why" should find non-direct dependency', async (t) => {
  prepare(t, {
    dependencies: {
      'dep-of-pkg-with-1-dep': '100.0.0',
      'pkg-with-1-dep': '100.0.0',
    },
  })

  await execa('pnpm', ['install', '--registry', `http://localhost:${REGISTRY_MOCK_PORT}`])

  const output = await why.handler({
    dev: false,
    dir: process.cwd(),
    optional: false,
  }, ['dep-of-pkg-with-1-dep'])

  t.equal(stripAnsi(output), `Legend: production dependency, optional only, dev only

project@0.0.0 ${process.cwd()}

dependencies:
dep-of-pkg-with-1-dep 100.0.0
pkg-with-1-dep 100.0.0
└── dep-of-pkg-with-1-dep 100.0.0`, 'prints prod deps only')

  t.end()
})
