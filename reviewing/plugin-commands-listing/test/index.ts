/// <reference path="../../../__typings__/index.d.ts" />
import fs from 'fs'
import path from 'path'
import { WANTED_LOCKFILE } from '@pnpm/constants'
import { list, why } from '@pnpm/plugin-commands-listing'
import { prepare, preparePackages } from '@pnpm/prepare'

import execa from 'execa'
import stripAnsi from 'strip-ansi'
import { sync as writeYamlFile } from 'write-yaml-file'

const pnpmBin = path.join(__dirname, '../../../pnpm/bin/pnpm.cjs')

test('listing packages', async () => {
  prepare({
    dependencies: {
      'is-positive': '1.0.0',
    },
    devDependencies: {
      'is-negative': '1.0.0',
    },
  })

  await execa('node', [pnpmBin, 'install'])

  {
    const output = await list.handler({
      dev: false,
      dir: process.cwd(),
      optional: false,
      virtualStoreDirMaxLength: 120,
    }, [])

    expect(stripAnsi(output)).toBe(`Legend: production dependency, optional only, dev only

project@0.0.0 ${process.cwd()}

dependencies:
is-positive 1.0.0`)
  }

  {
    const output = await list.handler({
      dir: process.cwd(),
      optional: false,
      production: false,
      virtualStoreDirMaxLength: 120,
    }, [])

    expect(stripAnsi(output)).toBe(`Legend: production dependency, optional only, dev only

project@0.0.0 ${process.cwd()}

devDependencies:
is-negative 1.0.0`)
  }

  {
    const output = await list.handler({
      dir: process.cwd(),
      virtualStoreDirMaxLength: 120,
    }, [])

    expect(stripAnsi(output)).toBe(`Legend: production dependency, optional only, dev only

project@0.0.0 ${process.cwd()}

dependencies:
is-positive 1.0.0

devDependencies:
is-negative 1.0.0`)
  }
})

test(`listing packages of a project that has an external ${WANTED_LOCKFILE}`, async () => {
  preparePackages([
    {
      name: 'pkg',
      version: '1.0.0',

      dependencies: {
        'is-positive': '1.0.0',
      },
    },
  ])

  writeYamlFile('pnpm-workspace.yaml', { packages: ['**', '!store/**'] })
  fs.writeFileSync('.npmrc', 'shared-workspace-lockfile = true', 'utf8')

  await execa('node', [pnpmBin, 'recursive', 'install'])

  process.chdir('pkg')

  const output = await list.handler({
    dir: process.cwd(),
    lockfileDir: path.resolve('..'),
    virtualStoreDirMaxLength: 120,
  }, [])

  expect(stripAnsi(output)).toBe(`Legend: production dependency, optional only, dev only

pkg@1.0.0 ${process.cwd()}

dependencies:
is-positive 1.0.0`)
})

// Use a preinstalled fixture
// Otherwise, we'd need to run the registry mock
test.skip('list on a project with skipped optional dependencies', async () => {
  prepare()

  await execa('node', [pnpmBin, 'add', '--no-optional', 'pkg-with-optional', 'is-positive@1.0.0'])

  {
    const output = await list.handler({
      depth: 10,
      dir: process.cwd(),
      virtualStoreDirMaxLength: 120,
    }, [])

    expect(stripAnsi(output)).toBe(`Legend: production dependency, optional only, dev only

project@0.0.0 ${process.cwd()}

dependencies:
is-positive 1.0.0
pkg-with-optional 1.0.0
└── not-compatible-with-any-os 1.0.0 skipped`)
  }

  {
    const output = await list.handler({
      depth: 10,
      dir: process.cwd(),
      virtualStoreDirMaxLength: 120,
    }, ['not-compatible-with-any-os'])

    expect(stripAnsi(output)).toBe(`Legend: production dependency, optional only, dev only

project@0.0.0 ${process.cwd()}

dependencies:
pkg-with-optional 1.0.0
└── not-compatible-with-any-os 1.0.0 skipped`)
  }

  {
    const output = await why.handler({
      dir: process.cwd(),
      virtualStoreDirMaxLength: 120,
    }, ['not-compatible-with-any-os'])

    expect(stripAnsi(output)).toBe(`Legend: production dependency, optional only, dev only

project@0.0.0 ${process.cwd()}

dependencies:
pkg-with-optional 1.0.0
└── not-compatible-with-any-os 1.0.0 skipped`)
  }
})

// Covers https://github.com/pnpm/pnpm/issues/6873
test('listing packages should not fail on package that has local file directory in dependencies', async () => {
  preparePackages([
    {
      name: 'dep',
      version: '1.0.0',
    },
    {
      name: 'pkg',
      version: '1.0.0',

      dependencies: {
        dep: 'file:../dep',
      },
    },
  ])

  const pkgDir = path.resolve('pkg')
  await execa('node', [pnpmBin, 'install'], { cwd: pkgDir })

  const output = await list.handler({
    dev: false,
    dir: pkgDir,
    optional: false,
    virtualStoreDirMaxLength: 120,
  }, [])

  expect(stripAnsi(output)).toBe(`Legend: production dependency, optional only, dev only

pkg@1.0.0 ${pkgDir}

dependencies:
dep file:../dep`)
})
