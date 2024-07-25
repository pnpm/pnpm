import path from 'path'
import { filterPackagesFromDir } from '@pnpm/workspace.filter-packages-from-dir'
import { install } from '@pnpm/plugin-commands-installation'
import { outdated } from '@pnpm/plugin-commands-outdated'
import { preparePackages } from '@pnpm/prepare'
import stripAnsi from 'strip-ansi'
import { DEFAULT_OPTS } from './utils'

test('pnpm recursive outdated', async () => {
  preparePackages([
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
        'is-positive': '2.0.0',
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

  const { allProjects, selectedProjectsGraph } = await filterPackagesFromDir(process.cwd(), [])
  await install.handler({
    ...DEFAULT_OPTS,
    allProjects,
    dir: process.cwd(),
    recursive: true,
    selectedProjectsGraph,
    workspaceDir: process.cwd(),
  })

  {
    const { output, exitCode } = await outdated.handler({
      ...DEFAULT_OPTS,
      allProjects,
      dir: process.cwd(),
      recursive: true,
      selectedProjectsGraph,
    })

    expect(exitCode).toBe(1)
    expect(stripAnsi(output as unknown as string)).toBe(`\
┌───────────────────┬─────────┬────────┬──────────────────────┐
│ Package           │ Current │ Latest │ Dependents           │
├───────────────────┼─────────┼────────┼──────────────────────┤
│ is-negative       │ 1.0.0   │ 2.1.0  │ project-2            │
├───────────────────┼─────────┼────────┼──────────────────────┤
│ is-negative (dev) │ 1.0.0   │ 2.1.0  │ project-3            │
├───────────────────┼─────────┼────────┼──────────────────────┤
│ is-positive       │ 1.0.0   │ 3.1.0  │ project-1, project-3 │
├───────────────────┼─────────┼────────┼──────────────────────┤
│ is-positive       │ 2.0.0   │ 3.1.0  │ project-2            │
└───────────────────┴─────────┴────────┴──────────────────────┘
`)
  }

  {
    const { output, exitCode } = await outdated.handler({
      ...DEFAULT_OPTS,
      allProjects,
      dir: process.cwd(),
      production: false,
      recursive: true,
      selectedProjectsGraph,
    })

    expect(exitCode).toBe(1)
    expect(stripAnsi(output as unknown as string)).toBe(`\
┌───────────────────┬─────────┬────────┬────────────┐
│ Package           │ Current │ Latest │ Dependents │
├───────────────────┼─────────┼────────┼────────────┤
│ is-negative (dev) │ 1.0.0   │ 2.1.0  │ project-3  │
└───────────────────┴─────────┴────────┴────────────┘
`)
  }

  {
    const { output, exitCode } = await outdated.handler({
      ...DEFAULT_OPTS,
      allProjects,
      dir: process.cwd(),
      long: true,
      recursive: true,
      selectedProjectsGraph,
    })

    expect(exitCode).toBe(1)
    expect(stripAnsi(output as unknown as string)).toBe(`\
┌───────────────────┬─────────┬────────┬──────────────────────┬─────────────────────────────────────────────┐
│ Package           │ Current │ Latest │ Dependents           │ Details                                     │
├───────────────────┼─────────┼────────┼──────────────────────┼─────────────────────────────────────────────┤
│ is-negative       │ 1.0.0   │ 2.1.0  │ project-2            │ https://github.com/kevva/is-negative#readme │
├───────────────────┼─────────┼────────┼──────────────────────┼─────────────────────────────────────────────┤
│ is-negative (dev) │ 1.0.0   │ 2.1.0  │ project-3            │ https://github.com/kevva/is-negative#readme │
├───────────────────┼─────────┼────────┼──────────────────────┼─────────────────────────────────────────────┤
│ is-positive       │ 1.0.0   │ 3.1.0  │ project-1, project-3 │ https://github.com/kevva/is-positive#readme │
├───────────────────┼─────────┼────────┼──────────────────────┼─────────────────────────────────────────────┤
│ is-positive       │ 2.0.0   │ 3.1.0  │ project-2            │ https://github.com/kevva/is-positive#readme │
└───────────────────┴─────────┴────────┴──────────────────────┴─────────────────────────────────────────────┘
`)
  }

  {
    const { output, exitCode } = await outdated.handler({
      ...DEFAULT_OPTS,
      allProjects,
      dir: process.cwd(),
      format: 'list',
      recursive: true,
      selectedProjectsGraph,
    })

    expect(exitCode).toBe(1)
    expect(stripAnsi(output as unknown as string)).toBe(`\
is-negative
1.0.0 => 2.1.0
Dependent: project-2

is-negative (dev)
1.0.0 => 2.1.0
Dependent: project-3

is-positive
1.0.0 => 3.1.0
Dependents: project-1, project-3

is-positive
2.0.0 => 3.1.0
Dependent: project-2
`)
  }

  {
    const { output, exitCode } = await outdated.handler({
      ...DEFAULT_OPTS,
      allProjects,
      dir: process.cwd(),
      recursive: true,
      selectedProjectsGraph,
      format: 'json',
    })

    expect(exitCode).toBe(1)
    expect(stripAnsi(output as unknown as string)).toBe(JSON.stringify({
      'is-negative': {
        current: '1.0.0',
        latest: '2.1.0',
        wanted: '1.0.0',
        isDeprecated: false,
        dependencyType: 'devDependencies',
        dependentPackages: [
          {
            name: 'project-3',
            location: path.resolve('project-3'),
          },
        ],
      },
      'is-positive': {
        current: '2.0.0',
        latest: '3.1.0',
        wanted: '2.0.0',
        isDeprecated: false,
        dependencyType: 'dependencies',
        dependentPackages: [
          {
            name: 'project-2',
            location: path.resolve('project-2'),
          },
        ],
      },
    }, null, 2))
  }

  {
    const { output, exitCode } = await outdated.handler({
      ...DEFAULT_OPTS,
      allProjects,
      dir: process.cwd(),
      format: 'list',
      long: true,
      recursive: true,
      selectedProjectsGraph,
    })

    expect(exitCode).toBe(1)
    expect(stripAnsi(output as unknown as string)).toBe(`\
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

is-positive
2.0.0 => 3.1.0
Dependent: project-2
https://github.com/kevva/is-positive#readme
`)
  }

  {
    const { output, exitCode } = await outdated.handler({
      ...DEFAULT_OPTS,
      allProjects,
      dir: process.cwd(),
      recursive: true,
      selectedProjectsGraph,
    }, ['is-positive'])

    expect(exitCode).toBe(1)
    expect(stripAnsi(output as unknown as string)).toBe(`\
┌─────────────┬─────────┬────────┬──────────────────────┐
│ Package     │ Current │ Latest │ Dependents           │
├─────────────┼─────────┼────────┼──────────────────────┤
│ is-positive │ 1.0.0   │ 3.1.0  │ project-1, project-3 │
├─────────────┼─────────┼────────┼──────────────────────┤
│ is-positive │ 2.0.0   │ 3.1.0  │ project-2            │
└─────────────┴─────────┴────────┴──────────────────────┘
`)
  }
})

test('pnpm recursive outdated: format json when there are no outdated dependencies', async () => {
  preparePackages([
    {
      name: 'project-1',
      version: '1.0.0',
    },
    {
      name: 'project-2',
      version: '1.0.0',
    },
    {
      name: 'project-3',
      version: '1.0.0',
    },
  ])

  const { allProjects, selectedProjectsGraph } = await filterPackagesFromDir(process.cwd(), [])
  const { output, exitCode } = await outdated.handler({
    ...DEFAULT_OPTS,
    allProjects,
    dir: process.cwd(),
    format: 'json',
    recursive: true,
    selectedProjectsGraph,
  })

  expect(exitCode).toBe(0)
  expect(stripAnsi(output)).toBe('{}')
})

test('pnpm recursive outdated in workspace with shared lockfile', async () => {
  preparePackages([
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

  const { allProjects, selectedProjectsGraph } = await filterPackagesFromDir(process.cwd(), [])
  await install.handler({
    ...DEFAULT_OPTS,
    allProjects,
    dir: process.cwd(),
    recursive: true,
    selectedProjectsGraph,
    workspaceDir: process.cwd(),
  })

  {
    const { output, exitCode } = await outdated.handler({
      ...DEFAULT_OPTS,
      allProjects,
      dir: process.cwd(),
      recursive: true,
      selectedProjectsGraph,
    })

    expect(exitCode).toBe(1)
    expect(stripAnsi(output as unknown as string)).toBe(`\
┌───────────────────┬─────────┬────────┬──────────────────────┐
│ Package           │ Current │ Latest │ Dependents           │
├───────────────────┼─────────┼────────┼──────────────────────┤
│ is-negative       │ 1.0.0   │ 2.1.0  │ project-2            │
├───────────────────┼─────────┼────────┼──────────────────────┤
│ is-negative (dev) │ 1.0.0   │ 2.1.0  │ project-3            │
├───────────────────┼─────────┼────────┼──────────────────────┤
│ is-positive       │ 1.0.0   │ 3.1.0  │ project-1, project-3 │
└───────────────────┴─────────┴────────┴──────────────────────┘
`)
  }

  {
    const { output, exitCode } = await outdated.handler({
      ...DEFAULT_OPTS,
      allProjects,
      dir: process.cwd(),
      production: false,
      recursive: true,
      selectedProjectsGraph,
    })

    expect(exitCode).toBe(1)
    expect(stripAnsi(output as unknown as string)).toBe(`\
┌───────────────────┬─────────┬────────┬────────────┐
│ Package           │ Current │ Latest │ Dependents │
├───────────────────┼─────────┼────────┼────────────┤
│ is-negative (dev) │ 1.0.0   │ 2.1.0  │ project-3  │
└───────────────────┴─────────┴────────┴────────────┘
`)
  }

  {
    const { output, exitCode } = await outdated.handler({
      ...DEFAULT_OPTS,
      allProjects,
      dir: process.cwd(),
      recursive: true,
      selectedProjectsGraph,
    }, ['is-positive'])

    expect(exitCode).toBe(1)
    expect(stripAnsi(output as unknown as string)).toBe(`\
┌─────────────┬─────────┬────────┬──────────────────────┐
│ Package     │ Current │ Latest │ Dependents           │
├─────────────┼─────────┼────────┼──────────────────────┤
│ is-positive │ 1.0.0   │ 3.1.0  │ project-1, project-3 │
└─────────────┴─────────┴────────┴──────────────────────┘
`)
  }
})
