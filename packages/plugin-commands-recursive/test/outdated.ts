import { recursive } from '@pnpm/plugin-commands-recursive'
import { preparePackages } from '@pnpm/prepare'
import { stripIndent } from 'common-tags'
import stripAnsi = require('strip-ansi')
import test = require('tape')
import writeYamlFile = require('write-yaml-file')
import { DEFAULT_OPTS } from './utils'

test('pnpm recursive outdated', async (t) => {
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

  await recursive.handler(['install'], {
    ...DEFAULT_OPTS,
    dir: process.cwd(),
  })

  {
    const output = await recursive.handler(['outdated'], {
      ...DEFAULT_OPTS,
      dir: process.cwd(),
    })

    t.equal(stripAnsi(output as unknown as string), stripIndent`
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
    const output = await recursive.handler(['outdated'], {
      ...DEFAULT_OPTS,
      dir: process.cwd(),
      long: true,
    })

    t.equal(stripAnsi(output as unknown as string), stripIndent`
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
    const output = await recursive.handler(['outdated'], {
      ...DEFAULT_OPTS,
      dir: process.cwd(),
      table: false,
    })

    t.equal(stripAnsi(output as unknown as string), stripIndent`
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
    const output = await recursive.handler(['outdated'], {
      ...DEFAULT_OPTS,
      dir: process.cwd(),
      long: true,
      table: false,
    })

    t.equal(stripAnsi(output as unknown as string), stripIndent`
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
    const output = await recursive.handler(['outdated', 'is-positive'], {
      ...DEFAULT_OPTS,
      dir: process.cwd(),
    })

    t.equal(stripAnsi(output as unknown as string), stripIndent`
    ┌─────────────┬─────────┬────────┬──────────────────────┐
    │ Package     │ Current │ Latest │ Dependents           │
    ├─────────────┼─────────┼────────┼──────────────────────┤
    │ is-positive │ 1.0.0   │ 3.1.0  │ project-1, project-3 │
    └─────────────┴─────────┴────────┴──────────────────────┘
    ` + '\n')
  }
  t.end()
})

test('pnpm recursive outdated in workspace with shared lockfile', async (t) => {
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

  await recursive.handler(['install'], {
    ...DEFAULT_OPTS,
    dir: process.cwd(),
  })

  {
    const output = await recursive.handler(['outdated'], {
      ...DEFAULT_OPTS,
      dir: process.cwd(),
    })

    t.equal(stripAnsi(output as unknown as string), stripIndent`
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
    const output = await recursive.handler(['outdated', 'is-positive'], {
      ...DEFAULT_OPTS,
      dir: process.cwd(),
    })

    t.equal(stripAnsi(output as unknown as string), stripIndent`
    ┌─────────────┬─────────┬────────┬──────────────────────┐
    │ Package     │ Current │ Latest │ Dependents           │
    ├─────────────┼─────────┼────────┼──────────────────────┤
    │ is-positive │ 1.0.0   │ 3.1.0  │ project-1, project-3 │
    └─────────────┴─────────┴────────┴──────────────────────┘
    ` + '\n')
  }
  t.end()
})
