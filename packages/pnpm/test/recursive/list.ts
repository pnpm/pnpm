import { preparePackages } from '@pnpm/prepare'
import { addDistTag } from '@pnpm/registry-mock'
import { stripIndent } from 'common-tags'
import fs = require('mz/fs')
import path = require('path')
import tape = require('tape')
import promisifyTape from 'tape-promise'
import writeYamlFile = require('write-yaml-file')
import {
  execPnpm,
  execPnpmSync,
} from '../utils'

const test = promisifyTape(tape)
const testOnly = promisifyTape(tape.only)

test('recursive list', async (t: tape.Test) => {
  const projects = preparePackages(t, [
    {
      name: 'project-1',
      version: '1.0.0',

      dependencies: {
        'is-positive': '1.0.0',
      },
    },
    {
      name: 'project-2',
      version: '1.0.0',

      dependencies: {
        'is-negative': '1.0.0',
      },
    },
    {
      name: 'project-3',
      version: '1.0.0',
    },
  ])

  await execPnpm('recursive', 'install')

  const result = execPnpmSync('recursive', 'list')

  t.equal(result.status, 0)

  t.equal(result.stdout.toString(), stripIndent`
    Legend: production dependency, optional only, dev only

    project-1@1.0.0 ${path.resolve('project-1')}

    dependencies:
    is-positive 1.0.0

    Legend: production dependency, optional only, dev only

    project-2@1.0.0 ${path.resolve('project-2')}

    dependencies:
    is-negative 1.0.0
  ` + '\n')
})

test('recursive list with shared-workspace-lockfile', async (t: tape.Test) => {
  await addDistTag({ package: 'dep-of-pkg-with-1-dep', version: '100.1.0', distTag: 'latest' })
  const projects = preparePackages(t, [
    {
      name: 'project-1',
      version: '1.0.0',

      dependencies: {
        'pkg-with-1-dep': '100.0.0',
      },
    },
    {
      name: 'project-2',
      version: '1.0.0',

      dependencies: {
        'is-negative': '1.0.0',
      },
    },
    {
      name: 'project-3',
      version: '1.0.0',
    },
  ])

  await writeYamlFile('pnpm-workspace.yaml', { packages: ['**', '!store/**'] })
  await fs.writeFile('.npmrc', 'shared-workspace-lockfile = true', 'utf8')

  await execPnpm('recursive', 'install', '--store', 'store')

  const result = execPnpmSync('recursive', 'list')

  t.equal(result.status, 0)

  t.equal(result.stdout.toString(), stripIndent`
    Legend: production dependency, optional only, dev only

    project-1@1.0.0 ${path.resolve('project-1')}

    dependencies:
    pkg-with-1-dep 100.0.0
    └── dep-of-pkg-with-1-dep 100.1.0

    Legend: production dependency, optional only, dev only

    project-2@1.0.0 ${path.resolve('project-2')}

    dependencies:
    is-negative 1.0.0
  ` + '\n')
})

test('recursive list --filter', async (t: tape.Test) => {
  const projects = preparePackages(t, [
    {
      name: 'project-1',
      version: '1.0.0',

      dependencies: {
        'is-positive': '1.0.0',
        'project-2': '1.0.0',
      },
    },
    {
      name: 'project-2',
      version: '1.0.0',

      dependencies: {
        'is-negative': '1.0.0',
      },
    },
    {
      name: 'project-3',
      version: '1.0.0',

      dependencies: {
        'is-negative': '1.0.0',
        'is-positive': '1.0.0',
      },
    },
  ])

  await execPnpm('recursive', 'install')

  const result = execPnpmSync('recursive', 'list', '--filter', 'project-1...')

  t.equal(result.status, 0)

  t.equal(result.stdout.toString(), stripIndent`
    Legend: production dependency, optional only, dev only

    project-1@1.0.0 ${path.resolve('project-1')}

    dependencies:
    is-positive 1.0.0
    project-2 link:../project-2

    Legend: production dependency, optional only, dev only

    project-2@1.0.0 ${path.resolve('project-2')}

    dependencies:
    is-negative 1.0.0
  ` + '\n')
})
