import path from 'node:path'
import { stripVTControlCharacters as stripAnsi } from 'node:util'

import { expect, test } from '@jest/globals'
import { list, why } from '@pnpm/deps.inspection.commands'
import type { PnpmError } from '@pnpm/error'
import { install } from '@pnpm/installing.commands'
import { prepare, preparePackages } from '@pnpm/prepare'
import { addDistTag } from '@pnpm/testing.registry-mock'
import { filterProjectsBySelectorObjectsFromDir } from '@pnpm/workspace.projects-filter'
import { writeYamlFileSync } from 'write-yaml-file'

import { DEFAULT_OPTS } from './utils/index.js'

test('recursive list', async () => {
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
    },
  ])

  const { allProjects, selectedProjectsGraph } = await filterProjectsBySelectorObjectsFromDir(process.cwd(), [])
  await install.handler({
    ...DEFAULT_OPTS,
    allProjects,
    cacheDir: path.resolve('cache'),
    dir: process.cwd(),
    recursive: true,
    selectedProjectsGraph,
    workspaceDir: process.cwd(),
  })

  const output = await list.handler({
    ...DEFAULT_OPTS,
    allProjects,
    dir: process.cwd(),
    recursive: true,
    selectedProjectsGraph,
  }, [])

  expect(stripAnsi(output as unknown as string)).toBe(`Legend: production dependency, optional only, dev only

project-1@1.0.0 ${path.resolve('project-1')}
│
│   dependencies:
└── is-positive@1.0.0

1 package

Legend: production dependency, optional only, dev only

project-2@1.0.0 ${path.resolve('project-2')}
│
│   dependencies:
└── is-negative@1.0.0

1 package`)
})

test('recursive list with sharedWorkspaceLockfile', async () => {
  await addDistTag({ package: '@pnpm.e2e/dep-of-pkg-with-1-dep', version: '100.1.0', distTag: 'latest' })
  preparePackages([
    {
      name: 'project-1',
      version: '1.0.0',

      dependencies: {
        '@pnpm.e2e/pkg-with-1-dep': '100.0.0',
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

  writeYamlFileSync('pnpm-workspace.yaml', {
    packages: ['**', '!store/**'],
    sharedWorkspaceLockfile: true,
  })

  const { allProjects, selectedProjectsGraph } = await filterProjectsBySelectorObjectsFromDir(process.cwd(), [])
  await install.handler({
    ...DEFAULT_OPTS,
    allProjects,
    cacheDir: path.resolve('cache'),
    dir: process.cwd(),
    recursive: true,
    selectedProjectsGraph,
    workspaceDir: process.cwd(),
  })

  const output = await list.handler({
    ...DEFAULT_OPTS,
    allProjects,
    cliOptions: { depth: 2 },
    dir: process.cwd(),
    recursive: true,
    selectedProjectsGraph,
  }, [])

  expect(stripAnsi(output as unknown as string)).toBe(`Legend: production dependency, optional only, dev only

project-1@1.0.0 ${path.resolve('project-1')}
│
│   dependencies:
└─┬ @pnpm.e2e/pkg-with-1-dep@100.0.0
  └── @pnpm.e2e/dep-of-pkg-with-1-dep@100.1.0

2 packages

Legend: production dependency, optional only, dev only

project-2@1.0.0 ${path.resolve('project-2')}
│
│   dependencies:
└── is-negative@1.0.0

1 package`)
})

test('recursive list --only-projects prints a selected project without project dependencies', async () => {
  preparePackages([
    {
      name: 'project-1',
      version: '1.0.0',

      dependencies: {
        'project-2': 'workspace:*',
      },
    },
    {
      name: 'project-2',
      version: '1.0.0',

      dependencies: {
        'is-positive': '1.0.0',
      },
    },
  ])
  writeYamlFileSync('pnpm-workspace.yaml', {
    packages: ['**', '!store/**'],
    sharedWorkspaceLockfile: true,
  })

  const { allProjects, selectedProjectsGraph } = await filterProjectsBySelectorObjectsFromDir(process.cwd(), [])
  await install.handler({
    ...DEFAULT_OPTS,
    allProjects,
    cacheDir: path.resolve('cache'),
    dir: process.cwd(),
    lockfileDir: process.cwd(),
    recursive: true,
    selectedProjectsGraph,
    workspaceDir: process.cwd(),
  })

  const listProjects = async (namePattern: string, params: string[] = []) => list.handler({
    ...DEFAULT_OPTS,
    cliOptions: { 'only-projects': true },
    dir: process.cwd(),
    lockfileDir: process.cwd(),
    parseable: true,
    recursive: true,
    ...await filterProjectsBySelectorObjectsFromDir(process.cwd(), [{ namePattern }]),
  }, params)

  expect(await listProjects('project-2')).toBe(path.resolve('project-2'))
  expect(await listProjects('project-1')).toBe(`${path.resolve('project-1')}
${path.resolve('project-2')}`)
  expect(await listProjects('project-2', ['project-1'])).toBe('')
})

test('recursive list --only-projects follows projects with dedicated lockfiles', async () => {
  preparePackages([
    {
      name: 'project-1',
      version: '1.0.0',

      dependencies: {
        'project-2': 'workspace:*',
      },
    },
    {
      name: 'project-2',
      version: '1.0.0',

      dependencies: {
        'project-3': 'workspace:*',
      },
    },
    {
      name: 'project-3',
      version: '1.0.0',
    },
  ])

  const { allProjects, selectedProjectsGraph } = await filterProjectsBySelectorObjectsFromDir(process.cwd(), [])
  await install.handler({
    ...DEFAULT_OPTS,
    allProjects,
    cacheDir: path.resolve('cache'),
    dir: process.cwd(),
    recursive: true,
    selectedProjectsGraph,
    workspaceDir: process.cwd(),
  })

  const output = await list.handler({
    ...DEFAULT_OPTS,
    cliOptions: { depth: Infinity, 'only-projects': true },
    dir: process.cwd(),
    recursive: true,
    ...await filterProjectsBySelectorObjectsFromDir(process.cwd(), [
      { namePattern: 'project-1' },
    ]),
  }, [])

  expect(stripAnsi(output as unknown as string)).toBe(`Legend: production dependency, optional only, dev only

project-1@1.0.0 ${path.resolve('project-1')}
│
│   dependencies:
└─┬ project-2@link:../project-2
  └── project-3@link:../project-3

2 packages`)
})

test.each([
  { sharedWorkspaceLockfile: false },
  { sharedWorkspaceLockfile: true },
])('list --only-projects follows a project linked through its publish directory (%o)', async ({ sharedWorkspaceLockfile }) => {
  preparePackages([
    {
      name: 'project-1',
      version: '1.0.0',

      dependencies: {
        'project-2': 'workspace:*',
      },
    },
    {
      name: 'project-2',
      version: '1.0.0',

      dependencies: {
        'project-3': 'workspace:*',
      },
      publishConfig: {
        directory: 'dist',
      },
    },
    {
      name: 'project-3',
      version: '1.0.0',
    },
  ])
  const lockfileDir = sharedWorkspaceLockfile ? process.cwd() : undefined

  const { allProjects, selectedProjectsGraph } = await filterProjectsBySelectorObjectsFromDir(process.cwd(), [])
  await install.handler({
    ...DEFAULT_OPTS,
    allProjects,
    cacheDir: path.resolve('cache'),
    dir: process.cwd(),
    lockfileDir,
    recursive: true,
    selectedProjectsGraph,
    sharedWorkspaceLockfile,
    workspaceDir: process.cwd(),
  })

  const listOpts = {
    ...DEFAULT_OPTS,
    dir: process.cwd(),
    lockfileDir,
    recursive: true,
    ...await filterProjectsBySelectorObjectsFromDir(process.cwd(), [
      { namePattern: 'project-1' },
    ]),
  }
  const output = await list.handler({
    ...listOpts,
    cliOptions: { depth: Infinity, 'only-projects': true },
  }, [])

  expect(stripAnsi(output as unknown as string)).toBe(`Legend: production dependency, optional only, dev only

project-1@1.0.0 ${path.resolve('project-1')}
│
│   dependencies:
└─┬ project-2@link:../project-2/dist
  └── project-3@link:../project-3

2 packages`)

  const parseable = await list.handler({
    ...listOpts,
    cliOptions: { depth: Infinity, 'only-projects': true, parseable: true },
    parseable: true,
  }, [])

  expect(parseable).toBe(['project-1', 'project-2', 'project-3'].map((project) => path.resolve(project)).join('\n'))
})

test('list --only-projects keeps a project directory that another project publishes into', async () => {
  preparePackages([
    {
      name: 'project-1',
      version: '1.0.0',

      dependencies: {
        'project-2': 'workspace:*',
      },
    },
    {
      name: 'project-2',
      version: '1.0.0',
    },
    {
      name: 'project-3',
      version: '1.0.0',

      dependencies: {
        'project-4': 'workspace:*',
      },
      publishConfig: {
        directory: '../project-2',
      },
    },
    {
      name: 'project-4',
      version: '1.0.0',
    },
  ])

  const { allProjects, selectedProjectsGraph } = await filterProjectsBySelectorObjectsFromDir(process.cwd(), [])
  await install.handler({
    ...DEFAULT_OPTS,
    allProjects,
    cacheDir: path.resolve('cache'),
    dir: process.cwd(),
    recursive: true,
    selectedProjectsGraph,
    sharedWorkspaceLockfile: false,
    workspaceDir: process.cwd(),
  })

  const output = await list.handler({
    ...DEFAULT_OPTS,
    dir: process.cwd(),
    recursive: true,
    ...await filterProjectsBySelectorObjectsFromDir(process.cwd(), [
      { namePattern: 'project-1' },
    ]),
    cliOptions: { depth: Infinity, 'only-projects': true },
  }, [])

  expect(stripAnsi(output as unknown as string)).toBe(`Legend: production dependency, optional only, dev only

project-1@1.0.0 ${path.resolve('project-1')}
│
│   dependencies:
└── project-2@link:../project-2

1 package`)
})

test('recursive list --filter', async () => {
  preparePackages([
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

  await install.handler({
    ...DEFAULT_OPTS,
    ...await filterProjectsBySelectorObjectsFromDir(process.cwd(), []),
    cacheDir: path.resolve('cache'),
    dir: process.cwd(),
    recursive: true,
    workspaceDir: process.cwd(),
  })

  const output = await list.handler({
    ...DEFAULT_OPTS,
    dir: process.cwd(),
    recursive: true,
    ...await filterProjectsBySelectorObjectsFromDir(process.cwd(), [
      { includeDependencies: true, namePattern: 'project-1' },
    ]),
  }, [])

  expect(stripAnsi(output as unknown as string)).toBe(`Legend: production dependency, optional only, dev only

project-1@1.0.0 ${path.resolve('project-1')}
│
│   dependencies:
├── is-positive@1.0.0
└── project-2@link:../project-2

2 packages

Legend: production dependency, optional only, dev only

project-2@1.0.0 ${path.resolve('project-2')}
│
│   dependencies:
└── is-negative@1.0.0

1 package`)
})

test('recursive list --filter link-workspace-packages=false', async () => {
  preparePackages([
    {
      dependencies: {
        'is-positive': '1.0.0',
        'project-2': 'workspace:*',
      },
      name: 'project-1',
      version: '1.0.0',
    },
    {
      name: 'project-2',
      version: '1.0.0',
    },
    {
      name: 'is-positive',
      version: '1.0.0',
    },
  ])

  await install.handler({
    ...DEFAULT_OPTS,
    ...await filterProjectsBySelectorObjectsFromDir(process.cwd(), [], { linkWorkspacePackages: false }),
    cacheDir: path.resolve('cache'),
    dir: process.cwd(),
    linkWorkspacePackages: false,
    recursive: true,
    workspaceDir: process.cwd(),
  })

  const output = await list.handler({
    ...DEFAULT_OPTS,
    dir: process.cwd(),
    recursive: true,
    ...await filterProjectsBySelectorObjectsFromDir(process.cwd(), [
      { includeDependencies: true, namePattern: 'project-1' },
    ], { linkWorkspacePackages: false }),
  }, [])

  expect(stripAnsi(output as unknown as string)).toBe(`Legend: production dependency, optional only, dev only

project-1@1.0.0 ${path.resolve('project-1')}
│
│   dependencies:
├── is-positive@1.0.0
└── project-2@link:../project-2

2 packages`)
})

test('`pnpm recursive why` should fail if no package name was provided', async () => {
  prepare()

  let err!: PnpmError
  try {
    await why.handler({
      ...DEFAULT_OPTS,
      ...await filterProjectsBySelectorObjectsFromDir(process.cwd(), []),
      dir: process.cwd(),
      recursive: true,
    }, [])
  } catch (_err: any) { // eslint-disable-line
    err = _err
  }

  expect(err.code).toBe('ERR_PNPM_MISSING_PACKAGE_NAME')
  expect(err.message).toMatch('`pnpm why` requires the package name')
})
