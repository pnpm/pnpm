import PnpmError from '@pnpm/error'
import { readProjects } from '@pnpm/filter-workspace-packages'
import { install } from '@pnpm/plugin-commands-installation'
import { list, why } from '@pnpm/plugin-commands-listing'
import prepare, { preparePackages } from '@pnpm/prepare'
import { addDistTag } from '@pnpm/registry-mock'
import { stripIndent } from 'common-tags'
import fs = require('mz/fs')
import path = require('path')
import stripAnsi = require('strip-ansi')
import test = require('tape')
import writeYamlFile = require('write-yaml-file')
import { DEFAULT_OPTS } from './utils'

test('recursive list', async (t) => {
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

  const { allProjects, selectedProjectsGraph } = await readProjects(process.cwd(), [])
  await install.handler([], {
    ...DEFAULT_OPTS,
    allProjects,
    dir: process.cwd(),
    recursive: true,
    selectedProjectsGraph,
    workspaceDir: process.cwd(),
  }, 'install')

  const output = await list.handler([], {
    ...DEFAULT_OPTS,
    allProjects,
    dir: process.cwd(),
    recursive: true,
    selectedProjectsGraph,
  }, 'list')

  t.equal(stripAnsi(output as unknown as string), stripIndent`
    Legend: production dependency, optional only, dev only

    project-1@1.0.0 ${path.resolve('project-1')}

    dependencies:
    is-positive 1.0.0

    Legend: production dependency, optional only, dev only

    project-2@1.0.0 ${path.resolve('project-2')}

    dependencies:
    is-negative 1.0.0
  `)

  t.end()
})

test('recursive list with shared-workspace-lockfile', async (t) => {
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

  const { allProjects, selectedProjectsGraph } = await readProjects(process.cwd(), [])
  await install.handler([], {
    ...DEFAULT_OPTS,
    allProjects,
    dir: process.cwd(),
    recursive: true,
    selectedProjectsGraph,
    workspaceDir: process.cwd(),
  }, 'install')

  const output = await list.handler([], {
    ...DEFAULT_OPTS,
    allProjects,
    depth: 2,
    dir: process.cwd(),
    recursive: true,
    selectedProjectsGraph,
  }, 'list')

  t.equal(stripAnsi(output as unknown as string), stripIndent`
    Legend: production dependency, optional only, dev only

    project-1@1.0.0 ${path.resolve('project-1')}

    dependencies:
    pkg-with-1-dep 100.0.0
    └── dep-of-pkg-with-1-dep 100.1.0

    Legend: production dependency, optional only, dev only

    project-2@1.0.0 ${path.resolve('project-2')}

    dependencies:
    is-negative 1.0.0
  `)
  t.end()
})

test('recursive list --filter', async (t) => {
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

  await install.handler([], {
    ...DEFAULT_OPTS,
    ...await readProjects(process.cwd(), []),
    dir: process.cwd(),
    recursive: true,
    workspaceDir: process.cwd(),
  }, 'install')

  const output = await list.handler([], {
    ...DEFAULT_OPTS,
    dir: process.cwd(),
    recursive: true,
    ...await readProjects(process.cwd(), [
      { includeDependencies: true, namePattern: 'project-1' },
    ]),
  }, 'list')

  t.equal(stripAnsi(output as unknown as string), stripIndent`
    Legend: production dependency, optional only, dev only

    project-1@1.0.0 ${path.resolve('project-1')}

    dependencies:
    is-positive 1.0.0
    project-2 link:../project-2

    Legend: production dependency, optional only, dev only

    project-2@1.0.0 ${path.resolve('project-2')}

    dependencies:
    is-negative 1.0.0
  `)
  t.end()
})

test('`pnpm recursive why` should fail if no package name was provided', async (t) => {
  prepare(t)

  let err!: PnpmError
  try {
    await why.handler([], {
      ...DEFAULT_OPTS,
      ...await readProjects(process.cwd(), []),
      dir: process.cwd(),
      recursive: true,
    }, 'why')
  } catch (_err) {
    err = _err
  }

  t.equal(err.code, 'ERR_PNPM_MISSING_PACKAGE_NAME')
  t.ok(err.message, '`pnpm why` requires the package name')
  t.end()
})
