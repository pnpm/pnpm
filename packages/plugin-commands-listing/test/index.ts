///<reference path="../../../typings/index.d.ts" />
import { WANTED_LOCKFILE } from '@pnpm/constants'
import PnpmError from '@pnpm/error'
import { list, why } from '@pnpm/plugin-commands-listing'
import prepare, { preparePackages } from '@pnpm/prepare'
import { stripIndent } from 'common-tags'
import execa = require('execa')
import fs = require('mz/fs')
import path = require('path')
import stripAnsi = require('strip-ansi')
import test = require('tape')
import writeYamlFile = require('write-yaml-file')
import './recursive'

test('listing packages', async (t) => {
  prepare(t, {
    dependencies: {
      'is-positive': '1.0.0',
    },
    devDependencies: {
      'is-negative': '1.0.0',
    },
  })

  await execa('pnpm', ['install'])

  {
    const output = await list.handler([], {
      dir: process.cwd(),
      include: { dependencies: true, devDependencies: false, optionalDependencies: false },
    })

    t.equal(stripAnsi(output), stripIndent`
      Legend: production dependency, optional only, dev only

      project@0.0.0 ${process.cwd()}

      dependencies:
      is-positive 1.0.0
    `, 'prints prod deps only')
  }

  {
    const output = await list.handler([], {
      dir: process.cwd(),
      include: { dependencies: false, devDependencies: true, optionalDependencies: false },
    })

    t.equal(stripAnsi(output), stripIndent`
      Legend: production dependency, optional only, dev only

      project@0.0.0 ${process.cwd()}

      devDependencies:
      is-negative 1.0.0
    `, 'prints dev deps only')
  }

  {
    const output = await list.handler([], {
      dir: process.cwd(),
      include: { dependencies: true, devDependencies: true, optionalDependencies: true },
    })

    t.equal(stripAnsi(output), stripIndent`
      Legend: production dependency, optional only, dev only

      project@0.0.0 ${process.cwd()}

      dependencies:
      is-positive 1.0.0

      devDependencies:
      is-negative 1.0.0
    `, 'prints all deps')
  }
  t.end()
})

test('independent-leaves=true: pnpm list --long', async (t) => {
  prepare(t, {
    dependencies: {
      'is-positive': '1.0.0',
    },
    devDependencies: {
      'is-negative': '1.0.0',
    },
  })

  await execa('pnpm', ['install', '--independent-leaves', '--no-hoist'])

  const output = await list.handler([], {
    dir: process.cwd(),
    include: { dependencies: true, devDependencies: true, optionalDependencies: true },
    long: true,
  })

  // TODO: the --long flag should work with --independent-leaves
  t.equal(stripAnsi(output), stripIndent`
    Legend: production dependency, optional only, dev only

    project@0.0.0 ${process.cwd()}

    dependencies:
    is-positive 1.0.0
      [Could not find additional info about this dependency]

    devDependencies:
    is-negative 1.0.0
      [Could not find additional info about this dependency]
  `)
  t.end()
})

test(`listing packages of a project that has an external ${WANTED_LOCKFILE}`, async (t) => {
  preparePackages(t, [
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

  await execa('pnpm', ['recursive', 'install'])

  process.chdir('pkg')

  const output = await list.handler([], {
    dir: process.cwd(),
    include: { dependencies: true, devDependencies: true, optionalDependencies: true },
    lockfileDir: path.resolve('..'),
  })

  t.equal(stripAnsi(output), stripIndent`
    Legend: production dependency, optional only, dev only

    pkg@1.0.0 ${process.cwd()}

    dependencies:
    is-positive 1.0.0
  `, 'prints all deps')
  t.end()
})

// Use a preinstalled fixture
// Otherwise, we'd need to run the registry mock
test.skip('list on a project with skipped optional dependencies', async (t) => {
  prepare(t)

  await execa('pnpm', ['add', '--no-optional', 'pkg-with-optional', 'is-positive@1.0.0'])

  {
    const output = await list.handler([], {
      depth: 10,
      dir: process.cwd(),
      include: { dependencies: true, devDependencies: true, optionalDependencies: true },
    })

    t.equal(stripAnsi(output), stripIndent`
      Legend: production dependency, optional only, dev only

      project@0.0.0 ${process.cwd()}

      dependencies:
      is-positive 1.0.0
      pkg-with-optional 1.0.0
      └── not-compatible-with-any-os 1.0.0 skipped
    `)
  }

  {
    const output = await list.handler(['not-compatible-with-any-os'], {
      depth: 10,
      dir: process.cwd(),
      include: { dependencies: true, devDependencies: true, optionalDependencies: true },
    })

    t.equal(stripAnsi(output), stripIndent`
      Legend: production dependency, optional only, dev only

      project@0.0.0 ${process.cwd()}

      dependencies:
      pkg-with-optional 1.0.0
      └── not-compatible-with-any-os 1.0.0 skipped
    `)
  }

  {
    const output = await why.handler(['not-compatible-with-any-os'], {
      dir: process.cwd(),
      include: { dependencies: true, devDependencies: true, optionalDependencies: true },
    })

    t.equal(stripAnsi(output), stripIndent`
      Legend: production dependency, optional only, dev only

      project@0.0.0 ${process.cwd()}

      dependencies:
      pkg-with-optional 1.0.0
      └── not-compatible-with-any-os 1.0.0 skipped
    `)
  }
  t.end()
})

test('`pnpm why` should fail if no package name was provided', async (t) => {
  prepare(t)

  let err!: PnpmError
  try {
    const output = await why.handler([], {
      dir: process.cwd(),
      include: { dependencies: true, devDependencies: true, optionalDependencies: true },
    })
  } catch (_err) {
    err = _err
  }

  t.equal(err.code, 'ERR_PNPM_MISSING_PACKAGE_NAME')
  t.ok(err.message.includes('`pnpm why` requires the package name'))
  t.end()
})
