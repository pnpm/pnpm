/// <reference path="../../../typings/index.d.ts" />
import { WANTED_LOCKFILE } from '@pnpm/constants'
import { list, why } from '@pnpm/plugin-commands-listing'
import prepare, { preparePackages } from '@pnpm/prepare'

import execa = require('execa')
import fs = require('mz/fs')
import path = require('path')
import stripAnsi = require('strip-ansi')
import writeYamlFile = require('write-yaml-file')

const pnpmBin = path.join(__dirname, '../../pnpm/bin/pnpm.js')

test('listing packages', async () => {
  prepare(undefined, {
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
    }, [])

    expect(stripAnsi(output)).toBe(`Legend: production dependency, optional only, dev only

project@0.0.0 ${process.cwd()}

devDependencies:
is-negative 1.0.0`)
  }

  {
    const output = await list.handler({
      dir: process.cwd(),
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
  preparePackages(undefined, [
    {
      name: 'pkg',
      version: '1.0.0',

      dependencies: {
        'is-positive': '1.0.0',
      },
    },
  ])

  await writeYamlFile('pnpm-workspace.yaml', { packages: ['**', '!store/**'] })
  await fs.writeFile('.npmrc', 'shared-workspace-lockfile = true', 'utf8')

  await execa('node', [pnpmBin, 'recursive', 'install'])

  process.chdir('pkg')

  const output = await list.handler({
    dir: process.cwd(),
    lockfileDir: path.resolve('..'),
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
    }, ['not-compatible-with-any-os'])

    expect(stripAnsi(output)).toBe(`Legend: production dependency, optional only, dev only

project@0.0.0 ${process.cwd()}

dependencies:
pkg-with-optional 1.0.0
└── not-compatible-with-any-os 1.0.0 skipped`)
  }
})
