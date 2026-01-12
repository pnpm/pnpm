import path from 'path'
import fs from 'fs'
import { type PnpmError } from '@pnpm/error'
import { why } from '@pnpm/plugin-commands-listing'
import { prepare } from '@pnpm/prepare'
import { REGISTRY_MOCK_PORT } from '@pnpm/registry-mock'
import execa from 'execa'
import { stripVTControlCharacters as stripAnsi } from 'util'

const pnpmBin = path.join(import.meta.dirname, '../../../pnpm/bin/pnpm.mjs')

test('`pnpm why` should fail if no package name was provided', async () => {
  prepare()

  let err!: PnpmError
  try {
    await why.handler({
      dir: process.cwd(),
      virtualStoreDirMaxLength: process.platform === 'win32' ? 60 : 120,
    }, [])
  } catch (_err: any) { // eslint-disable-line
    err = _err
  }

  expect(err.code).toBe('ERR_PNPM_MISSING_PACKAGE_NAME')
  expect(err.message).toMatch(/`pnpm why` requires the package name/)
})

test('"why" should find non-direct dependency', async () => {
  prepare({
    dependencies: {
      '@pnpm.e2e/dep-of-pkg-with-1-dep': '100.0.0',
      '@pnpm.e2e/pkg-with-1-dep': '100.0.0',
    },
  })

  await execa('node', [pnpmBin, 'install', '--registry', `http://localhost:${REGISTRY_MOCK_PORT}`])

  const output = await why.handler({
    dev: false,
    dir: process.cwd(),
    optional: false,
    virtualStoreDirMaxLength: process.platform === 'win32' ? 60 : 120,
  }, ['@pnpm.e2e/dep-of-pkg-with-1-dep'])

  expect(stripAnsi(output)).toBe(`Legend: production dependency, optional only, dev only

project@0.0.0 ${process.cwd()}

dependencies:
@pnpm.e2e/dep-of-pkg-with-1-dep 100.0.0
@pnpm.e2e/pkg-with-1-dep 100.0.0
└── @pnpm.e2e/dep-of-pkg-with-1-dep 100.0.0`)
})

test('"why" should find packages by alias name when using npm: protocol', async () => {
  prepare({
    dependencies: {
      foo: 'npm:@pnpm.e2e/pkg-with-1-dep@100.0.0',
    },
  })

  await execa('node', [pnpmBin, 'install', '--registry', `http://localhost:${REGISTRY_MOCK_PORT}`])

  const output = await why.handler({
    dev: false,
    dir: process.cwd(),
    optional: false,
    virtualStoreDirMaxLength: process.platform === 'win32' ? 60 : 120,
  }, ['foo'])

  const lines = stripAnsi(output).split('\n')
  expect(lines).toContain('foo npm:@pnpm.e2e/pkg-with-1-dep@100.0.0')
})

test('"why" should find packages by actual package name when using npm: protocol', async () => {
  prepare({
    dependencies: {
      foo: 'npm:@pnpm.e2e/pkg-with-1-dep@100.0.0',
    },
  })

  await execa('node', [pnpmBin, 'install', '--registry', `http://localhost:${REGISTRY_MOCK_PORT}`])

  const output = await why.handler({
    dev: false,
    dir: process.cwd(),
    optional: false,
    virtualStoreDirMaxLength: process.platform === 'win32' ? 60 : 120,
  }, ['@pnpm.e2e/pkg-with-1-dep'])

  const lines = stripAnsi(output).split('\n')
  expect(lines).toContain('foo npm:@pnpm.e2e/pkg-with-1-dep@100.0.0')
})

test('"why" should display npm: protocol in parseable format', async () => {
  prepare({
    dependencies: {
      foo: 'npm:@pnpm.e2e/pkg-with-1-dep@100.0.0',
    },
  })

  await execa('node', [pnpmBin, 'install', '--registry', `http://localhost:${REGISTRY_MOCK_PORT}`])

  const output = await why.handler({
    dev: false,
    dir: process.cwd(),
    optional: false,
    long: true,
    parseable: true,
    virtualStoreDirMaxLength: process.platform === 'win32' ? 60 : 120,
  }, ['foo'])

  const lines = output.split('\n')
  expect(lines.some(line => line.includes('foo npm:@pnpm.e2e/pkg-with-1-dep@100.0.0'))).toBe(true)
})

test('"why" should display file: protocol correctly for aliased packages', async () => {
  prepare({
    dependencies: {
      'my-alias': 'file:./local-pkg',
    },
  })

  // Create a local package after prepare() changes directory
  const localPkgDir = path.join(process.cwd(), 'local-pkg')
  fs.mkdirSync(localPkgDir, { recursive: true })
  fs.writeFileSync(
    path.join(localPkgDir, 'package.json'),
    JSON.stringify({ name: 'my-local-pkg', version: '1.0.0' })
  )

  await execa('node', [pnpmBin, 'install'])

  const output = await why.handler({
    dev: false,
    dir: process.cwd(),
    optional: false,
    virtualStoreDirMaxLength: process.platform === 'win32' ? 60 : 120,
  }, ['my-local-pkg'])

  const lines = stripAnsi(output).split('\n')
  expect(lines).toContain('my-alias my-local-pkg@file:local-pkg')
})
