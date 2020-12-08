import PnpmError from '@pnpm/error'
import { why } from '@pnpm/plugin-commands-listing'
import prepare from '@pnpm/prepare'
import { REGISTRY_MOCK_PORT } from '@pnpm/registry-mock'
import execa = require('execa')
import stripAnsi = require('strip-ansi')

test('`pnpm why` should fail if no package name was provided', async () => {
  prepare()

  let err!: PnpmError
  try {
    await why.handler({
      dir: process.cwd(),
    }, [])
  } catch (_err) {
    err = _err
  }

  expect(err.code).toBe('ERR_PNPM_MISSING_PACKAGE_NAME')
  expect(err.message).toMatch(/`pnpm why` requires the package name/)
})

test('"why" should find non-direct dependency', async () => {
  prepare(undefined, {
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

  expect(stripAnsi(output)).toBe(`Legend: production dependency, optional only, dev only

project@0.0.0 ${process.cwd()}

dependencies:
dep-of-pkg-with-1-dep 100.0.0
pkg-with-1-dep 100.0.0
└── dep-of-pkg-with-1-dep 100.0.0`)
})
