import PnpmError from '@pnpm/error'
import { readProjects } from '@pnpm/filter-workspace-packages'
import { install } from '@pnpm/plugin-commands-installation'
import { list, why } from '@pnpm/plugin-commands-listing'
import prepare, { preparePackages } from '@pnpm/prepare'
import { addDistTag } from '@pnpm/registry-mock'
import { DEFAULT_OPTS } from './utils'
import path = require('path')
import fs = require('mz/fs')
import stripAnsi = require('strip-ansi')
import writeYamlFile = require('write-yaml-file')

test('recursive list', async () => {
  preparePackages(undefined, [
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

  const { allProjects, selectedProjectsGraph } = await readProjects(process.cwd(), [])
  await install.handler({
    ...DEFAULT_OPTS,
    allProjects,
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

dependencies:
is-positive 1.0.0

Legend: production dependency, optional only, dev only

project-2@1.0.0 ${path.resolve('project-2')}

dependencies:
is-negative 1.0.0`)
})

test('recursive list with shared-workspace-lockfile', async () => {
  await addDistTag({ package: 'dep-of-pkg-with-1-dep', version: '100.1.0', distTag: 'latest' })
  preparePackages(undefined, [
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

  const { allProjects, selectedProjectsGraph } = await readProjects(process.cwd(), [])
  await install.handler({
    ...DEFAULT_OPTS,
    allProjects,
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

dependencies:
pkg-with-1-dep 100.0.0
└── dep-of-pkg-with-1-dep 100.1.0

Legend: production dependency, optional only, dev only

project-2@1.0.0 ${path.resolve('project-2')}

dependencies:
is-negative 1.0.0`)
})

test('recursive list --filter', async () => {
  preparePackages(undefined, [
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
    ...await readProjects(process.cwd(), []),
    dir: process.cwd(),
    recursive: true,
    workspaceDir: process.cwd(),
  })

  const output = await list.handler({
    ...DEFAULT_OPTS,
    dir: process.cwd(),
    recursive: true,
    ...await readProjects(process.cwd(), [
      { includeDependencies: true, namePattern: 'project-1' },
    ]),
  }, [])

  expect(stripAnsi(output as unknown as string)).toBe(`Legend: production dependency, optional only, dev only

project-1@1.0.0 ${path.resolve('project-1')}

dependencies:
is-positive 1.0.0
project-2 link:../project-2

Legend: production dependency, optional only, dev only

project-2@1.0.0 ${path.resolve('project-2')}

dependencies:
is-negative 1.0.0`)
})

test('recursive list --filter link-workspace-packages=false', async () => {
  preparePackages(undefined, [
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
    ...await readProjects(process.cwd(), [], { linkWorkspacePackages: false }),
    dir: process.cwd(),
    linkWorkspacePackages: false,
    recursive: true,
    workspaceDir: process.cwd(),
  })

  const output = await list.handler({
    ...DEFAULT_OPTS,
    dir: process.cwd(),
    recursive: true,
    ...await readProjects(process.cwd(), [
      { includeDependencies: true, namePattern: 'project-1' },
    ], { linkWorkspacePackages: false }),
  }, [])

  expect(stripAnsi(output as unknown as string)).toBe(`Legend: production dependency, optional only, dev only

project-1@1.0.0 ${path.resolve('project-1')}

dependencies:
is-positive 1.0.0
project-2 link:../project-2`)
})

test('`pnpm recursive why` should fail if no package name was provided', async () => {
  prepare()

  let err!: PnpmError
  try {
    await why.handler({
      ...DEFAULT_OPTS,
      ...await readProjects(process.cwd(), []),
      dir: process.cwd(),
      recursive: true,
    }, [])
  } catch (_err) {
    err = _err
  }

  expect(err.code).toBe('ERR_PNPM_MISSING_PACKAGE_NAME')
  expect(err.message).toBe('`pnpm why` requires the package name')
})
