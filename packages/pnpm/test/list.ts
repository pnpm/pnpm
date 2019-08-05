import { WANTED_LOCKFILE } from '@pnpm/constants'
import prepare, { preparePackages, tempDir } from '@pnpm/prepare'
import { stripIndent } from 'common-tags'
import isWindows = require('is-windows')
import fs = require('mz/fs')
import path = require('path')
import tape = require('tape')
import promisifyTape from 'tape-promise'
import writeYamlFile = require('write-yaml-file')
import {
  execPnpm,
  execPnpmSync,
} from './utils'

const test = promisifyTape(tape)
const testOnly = promisifyTape(tape.only)

test('listing global packages', async (t: tape.Test) => {
  tempDir(t)

  const global = path.resolve('global')

  if (process.env.APPDATA) process.env.APPDATA = global
  process.env.NPM_CONFIG_PREFIX = global

  await execPnpm('install', '-g', 'is-positive@3.1.0')

  const result = execPnpmSync('list', '-g')

  t.equal(result.status, 0)

  const globalPrefix = isWindows()
    ? path.join(global, 'npm', 'pnpm-global', '2')
    : path.join(global, 'pnpm-global', '2')
  t.equal(result.stdout.toString(), stripIndent`
    Legend: production dependency, optional only, dev only

    ${globalPrefix}

    dependencies:
    is-positive 3.1.0
  ` + '\n')
})

test('listing global packages installed with independent-leaves = true', async (t: tape.Test) => {
  tempDir(t)

  const global = path.resolve('global')

  if (process.env.APPDATA) process.env.APPDATA = global
  process.env.NPM_CONFIG_PREFIX = global

  await execPnpm('install', '-g', '--independent-leaves', 'is-positive@3.1.0')

  const result = execPnpmSync('list', '-g', '--independent-leaves')

  t.equal(result.status, 0)

  const globalPrefix = isWindows()
    ? path.join(global, 'npm', 'pnpm-global', '2_independent_leaves')
    : path.join(global, 'pnpm-global', '2_independent_leaves')
  t.equal(result.stdout.toString(), stripIndent`
    Legend: production dependency, optional only, dev only

    ${globalPrefix}

    dependencies:
    is-positive 3.1.0
  ` + '\n')
})

test('listing packages', async (t: tape.Test) => {
  prepare(t, {
    dependencies: {
      'is-positive': '1.0.0',
    },
    devDependencies: {
      'is-negative': '1.0.0',
    },
  })

  await execPnpm('install')

  {
    const result = execPnpmSync('list', '--prod')

    t.equal(result.status, 0)

    t.equal(result.stdout.toString(), stripIndent`
      Legend: production dependency, optional only, dev only

      project@0.0.0 ${process.cwd()}

      dependencies:
      is-positive 1.0.0
    ` + '\n', 'prints prod deps only')
  }

  {
    const result = execPnpmSync('list', '--only', 'prod')

    t.equal(result.status, 0)

    t.equal(result.stdout.toString(), stripIndent`
      Legend: production dependency, optional only, dev only

      project@0.0.0 ${process.cwd()}

      dependencies:
      is-positive 1.0.0
    ` + '\n', 'prints prod deps only using --only prod')
  }

  {
    const result = execPnpmSync('list', '--dev')

    t.equal(result.status, 0)

    t.equal(result.stdout.toString(), stripIndent`
      Legend: production dependency, optional only, dev only

      project@0.0.0 ${process.cwd()}

      devDependencies:
      is-negative 1.0.0
    ` + '\n', 'prints dev deps only')
  }

  {
    const result = execPnpmSync('list', '--only', 'dev')

    t.equal(result.status, 0)

    t.equal(result.stdout.toString(), stripIndent`
      Legend: production dependency, optional only, dev only

      project@0.0.0 ${process.cwd()}

      devDependencies:
      is-negative 1.0.0
    ` + '\n', 'prints dev deps only using --only dev')
  }

  {
    const result = execPnpmSync('list')

    t.equal(result.status, 0)

    t.equal(result.stdout.toString(), stripIndent`
      Legend: production dependency, optional only, dev only

      project@0.0.0 ${process.cwd()}

      dependencies:
      is-positive 1.0.0

      devDependencies:
      is-negative 1.0.0
    ` + '\n', 'prints all deps')
  }
})

test('independent-leaves=true: pnpm list --long', async (t: tape.Test) => {
  prepare(t, {
    dependencies: {
      'is-positive': '1.0.0',
    },
    devDependencies: {
      'is-negative': '1.0.0',
    },
  })

  await execPnpm('install', '--independent-leaves')

  const result = execPnpmSync('list', '--long')

  t.equal(result.status, 0)

  // TODO: the --long flag should work with --independent-leaves
  t.equal(result.stdout.toString(), stripIndent`
    Legend: production dependency, optional only, dev only

    project@0.0.0 ${process.cwd()}

    dependencies:
    is-positive 1.0.0
      [Could not find additional info about this dependency]

    devDependencies:
    is-negative 1.0.0
      [Could not find additional info about this dependency]
  ` + '\n')
})

test(`listing packages of a project that has an external ${WANTED_LOCKFILE}`, async (t: tape.Test) => {
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

  await execPnpm('recursive', 'install')

  process.chdir('pkg')

  const result = execPnpmSync('list')

  t.equal(result.status, 0)

  t.equal(result.stdout.toString(), stripIndent`
    Legend: production dependency, optional only, dev only

    pkg@1.0.0 ${process.cwd()}

    dependencies:
    is-positive 1.0.0
  ` + '\n', 'prints all deps')
})

test('list on a project with skipped optional dependencies', async (t: tape.Test) => {
  prepare(t)

  await execPnpm('add', '--no-optional', 'pkg-with-optional')

  const result = execPnpmSync('list', '--depth', '10')

  t.equal(result.status, 0)

  t.equal(result.stdout.toString(), stripIndent`
    Legend: production dependency, optional only, dev only

    project@0.0.0 /home/zoltan/src/pnpm/.tmp/1/project

    dependencies:
    pkg-with-optional 1.0.0
    └── not-compatible-with-any-os 1.0.0 skipped
  ` + '\n')
})
