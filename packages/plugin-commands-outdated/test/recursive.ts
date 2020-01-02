import { readProjects } from '@pnpm/filter-workspace-packages'
import { install } from '@pnpm/plugin-commands-installation'
import { outdated } from '@pnpm/plugin-commands-outdated'
import { preparePackages } from '@pnpm/prepare'
import { stripIndent } from 'common-tags'
import stripAnsi = require('strip-ansi')
import test = require('tape')
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

  const { allProjects, selectedProjectsGraph } = await readProjects(process.cwd(), [])
  await install.handler([], {
    ...DEFAULT_OPTS,
    allProjects,
    dir: process.cwd(),
    recursive: true,
    selectedProjectsGraph,
    workspaceDir: process.cwd(),
  })

  {
    const output = await outdated.handler([], {
      ...DEFAULT_OPTS,
      allProjects,
      dir: process.cwd(),
      recursive: true,
      selectedProjectsGraph,
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
    const output = await outdated.handler([], {
      ...DEFAULT_OPTS,
      allProjects,
      dir: process.cwd(),
      long: true,
      recursive: true,
      selectedProjectsGraph,
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
    const output = await outdated.handler([], {
      ...DEFAULT_OPTS,
      allProjects,
      dir: process.cwd(),
      recursive: true,
      selectedProjectsGraph,
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
    const output = await outdated.handler([], {
      ...DEFAULT_OPTS,
      allProjects,
      dir: process.cwd(),
      long: true,
      recursive: true,
      selectedProjectsGraph,
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
    const output = await outdated.handler(['is-positive'], {
      ...DEFAULT_OPTS,
      allProjects,
      dir: process.cwd(),
      recursive: true,
      selectedProjectsGraph,
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

  const { allProjects, selectedProjectsGraph } = await readProjects(process.cwd(), [])
  await install.handler([], {
    ...DEFAULT_OPTS,
    allProjects,
    dir: process.cwd(),
    recursive: true,
    selectedProjectsGraph,
    workspaceDir: process.cwd(),
  })

  {
    const output = await outdated.handler([], {
      ...DEFAULT_OPTS,
      allProjects,
      dir: process.cwd(),
      recursive: true,
      selectedProjectsGraph,
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
    const output = await outdated.handler(['is-positive'], {
      ...DEFAULT_OPTS,
      allProjects,
      dir: process.cwd(),
      recursive: true,
      selectedProjectsGraph,
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
