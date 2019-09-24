import { preparePackages } from '@pnpm/prepare'
import { stripIndent } from 'common-tags'
import normalizeNewline = require('normalize-newline')
import tape = require('tape')
import promisifyTape from 'tape-promise'
import writeYamlFile = require('write-yaml-file')
import {
  execPnpm,
  execPnpmSync,
} from '../utils'

const test = promisifyTape(tape)
const testOnly = promisifyTape(tape.only)

test('pnpm recursive outdated', async (t: tape.Test) => {
  preparePackages(t, [
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

      dependencies: {
        'is-positive': '1.0.0',
      },
      devDependencies: {
        'is-negative': '1.0.0',
      },
    },
  ])

  await execPnpm('recursive', 'install')

  {
    const result = execPnpmSync('recursive', 'outdated')

    t.equal(result.status, 0)

    t.equal(normalizeNewline(result.stdout.toString()), stripIndent`
    ┌───────────────────┬─────────┬────────┬──────────────────────┐
    │ Package           │ Current │ Latest │ Dependents           │
    ├───────────────────┼─────────┼────────┼──────────────────────┤
    │ is-negative       │ 1.0.0   │ 2.1.0  │ project-2            │
    ├───────────────────┼─────────┼────────┼──────────────────────┤
    │ is-negative (dev) │ 1.0.0   │ 2.1.0  │ project-3            │
    ├───────────────────┼─────────┼────────┼──────────────────────┤
    │ is-positive       │ 1.0.0   │ 3.1.0  │ project-1, project-3 │
    └───────────────────┴─────────┴────────┴──────────────────────┘
    ` + '\n')
  }

  {
    const result = execPnpmSync('recursive', 'outdated', '--long')

    t.equal(result.status, 0)

    t.equal(normalizeNewline(result.stdout.toString()), stripIndent`
    ┌───────────────────┬─────────┬────────┬──────────────────────┬─────────────────────────────────────────────┐
    │ Package           │ Current │ Latest │ Dependents           │ Details                                     │
    ├───────────────────┼─────────┼────────┼──────────────────────┼─────────────────────────────────────────────┤
    │ is-negative       │ 1.0.0   │ 2.1.0  │ project-2            │ https://github.com/kevva/is-negative#readme │
    ├───────────────────┼─────────┼────────┼──────────────────────┼─────────────────────────────────────────────┤
    │ is-negative (dev) │ 1.0.0   │ 2.1.0  │ project-3            │ https://github.com/kevva/is-negative#readme │
    ├───────────────────┼─────────┼────────┼──────────────────────┼─────────────────────────────────────────────┤
    │ is-positive       │ 1.0.0   │ 3.1.0  │ project-1, project-3 │ https://github.com/kevva/is-positive#readme │
    └───────────────────┴─────────┴────────┴──────────────────────┴─────────────────────────────────────────────┘
    ` + '\n')
  }

  {
    const result = execPnpmSync('recursive', 'outdated', '--no-table')

    t.equal(result.status, 0)

    t.equal(normalizeNewline(result.stdout.toString()), stripIndent`
    is-negative
    1.0.0 => 2.1.0
    Dependent: project-2

    is-negative (dev)
    1.0.0 => 2.1.0
    Dependent: project-3

    is-positive
    1.0.0 => 3.1.0
    Dependents: project-1, project-3
    ` + '\n')
  }

  {
    const result = execPnpmSync('recursive', 'outdated', '--no-table', '--long')

    t.equal(result.status, 0)

    t.equal(normalizeNewline(result.stdout.toString()), stripIndent`
    is-negative
    1.0.0 => 2.1.0
    Dependent: project-2
    https://github.com/kevva/is-negative#readme

    is-negative (dev)
    1.0.0 => 2.1.0
    Dependent: project-3
    https://github.com/kevva/is-negative#readme

    is-positive
    1.0.0 => 3.1.0
    Dependents: project-1, project-3
    https://github.com/kevva/is-positive#readme
    ` + '\n')
  }

  {
    const result = execPnpmSync('recursive', 'outdated', 'is-positive')

    t.equal(result.status, 0)

    t.equal(normalizeNewline(result.stdout.toString()), stripIndent`
    ┌─────────────┬─────────┬────────┬──────────────────────┐
    │ Package     │ Current │ Latest │ Dependents           │
    ├─────────────┼─────────┼────────┼──────────────────────┤
    │ is-positive │ 1.0.0   │ 3.1.0  │ project-1, project-3 │
    └─────────────┴─────────┴────────┴──────────────────────┘
    ` + '\n')
  }
})

test('pnpm recursive outdated in workspace with shared lockfile', async (t: tape.Test) => {
  preparePackages(t, [
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

      dependencies: {
        'is-positive': '1.0.0',
      },
      devDependencies: {
        'is-negative': '1.0.0',
      },
    },
  ])

  await writeYamlFile('pnpm-workspace.yaml', { packages: ['**', '!store/**'] })

  await execPnpm('recursive', 'install')

  {
    const result = execPnpmSync('recursive', 'outdated')

    t.equal(result.status, 0)

    t.equal(normalizeNewline(result.stdout.toString()), stripIndent`
    ┌───────────────────┬─────────┬────────┬──────────────────────┐
    │ Package           │ Current │ Latest │ Dependents           │
    ├───────────────────┼─────────┼────────┼──────────────────────┤
    │ is-negative       │ 1.0.0   │ 2.1.0  │ project-2            │
    ├───────────────────┼─────────┼────────┼──────────────────────┤
    │ is-negative (dev) │ 1.0.0   │ 2.1.0  │ project-3            │
    ├───────────────────┼─────────┼────────┼──────────────────────┤
    │ is-positive       │ 1.0.0   │ 3.1.0  │ project-1, project-3 │
    └───────────────────┴─────────┴────────┴──────────────────────┘
    ` + '\n')
  }

  {
    const result = execPnpmSync('recursive', 'outdated', 'is-positive')

    t.equal(result.status, 0)

    t.equal(normalizeNewline(result.stdout.toString()), stripIndent`
    ┌─────────────┬─────────┬────────┬──────────────────────┐
    │ Package     │ Current │ Latest │ Dependents           │
    ├─────────────┼─────────┼────────┼──────────────────────┤
    │ is-positive │ 1.0.0   │ 3.1.0  │ project-1, project-3 │
    └─────────────┴─────────┴────────┴──────────────────────┘
    ` + '\n')
  }
})
