import {stripIndent} from 'common-tags'
import tape = require('tape')
import promisifyTape from 'tape-promise'
import path = require('path')
import {
  execPnpm,
  execPnpmSync,
  preparePackages,
} from '../utils'

const test = promisifyTape(tape)

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
    project-1@1.0.0 ${path.resolve('project-1')}
    └── is-positive@1.0.0

    project-2@1.0.0 ${path.resolve('project-2')}
    └── is-negative@1.0.0
  ` + '\n\n')
})

test('recursive list --scope', async (t: tape.Test) => {
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

  await execPnpm('recursive', 'link')

  const result = execPnpmSync('recursive', 'list', '--scope', 'project-1')

  t.equal(result.status, 0)

  t.equal(result.stdout.toString(), stripIndent`
    project-1@1.0.0 ${path.resolve('project-1')}
    ├── is-positive@1.0.0
    └── project-2@link:../project-2

    project-2@1.0.0 ${path.resolve('project-2')}
    └── is-negative@1.0.0
  ` + '\n\n')
})
