import { promises as fs } from 'fs'
import { prepare, preparePackages } from '@pnpm/prepare'
import writeYamlFile from 'write-yaml-file'
import { execPnpm } from '../utils'

test('hoist the dependency graph', async () => {
  const project = prepare()

  await execPnpm(['install', 'express@4.16.2'])

  project.has('express')
  project.has('.pnpm/node_modules/debug')
  project.has('.pnpm/node_modules/cookie')

  await execPnpm(['uninstall', 'express'])

  project.hasNot('express')
  project.hasNot('.pnpm/node_modules/debug')
  project.hasNot('.pnpm/node_modules/cookie')
})

test('shamefully hoist the dependency graph', async () => {
  const project = prepare()

  await execPnpm(['add', '--shamefully-hoist', 'express@4.16.2'])

  project.has('express')
  project.has('debug')
  project.has('cookie')

  await execPnpm(['remove', 'express'])

  project.hasNot('express')
  project.hasNot('debug')
  project.hasNot('cookie')
})

test('shamefully-hoist: applied to all the workspace projects when set to true in the root .npmrc file', async () => {
  const projects = preparePackages([
    {
      location: '.',
      package: {
        name: 'root',

        dependencies: {
          '@pnpm.e2e/pkg-with-1-dep': '100.0.0',
        },
      },
    },
    {
      name: 'project',
      version: '1.0.0',

      dependencies: {
        '@pnpm.e2e/foobar': '100.0.0',
      },
    },
  ])

  await writeYamlFile('pnpm-workspace.yaml', { packages: ['**', '!store/**'] })
  await fs.writeFile('.npmrc', 'shamefully-hoist=true', 'utf8')

  await execPnpm(['install'])

  projects.root.has('@pnpm.e2e/dep-of-pkg-with-1-dep')
  projects.root.has('@pnpm.e2e/foo')
  projects.root.has('@pnpm.e2e/foobar')
  projects.project.hasNot('@pnpm.e2e/foo')
  projects.project.has('@pnpm.e2e/foobar')
})

test('shamefully-hoist: applied to all the workspace projects when set to true in the root .npmrc file (with dedupe-direct-deps=true)', async () => {
  const projects = preparePackages([
    {
      location: '.',
      package: {
        name: 'root',

        dependencies: {
          '@pnpm.e2e/pkg-with-1-dep': '100.0.0',
        },
      },
    },
    {
      name: 'project',
      version: '1.0.0',

      dependencies: {
        '@pnpm.e2e/foobar': '100.0.0',
      },
    },
  ])

  await writeYamlFile('pnpm-workspace.yaml', { packages: ['**', '!store/**'] })
  await fs.writeFile('.npmrc', `shamefully-hoist=true
dedupe-direct-deps=true`, 'utf8')

  await execPnpm(['install'])

  projects.root.has('@pnpm.e2e/dep-of-pkg-with-1-dep')
  projects.root.has('@pnpm.e2e/foo')
  projects.root.has('@pnpm.e2e/foobar')
  projects.project.hasNot('@pnpm.e2e/foo')
  projects.project.hasNot('@pnpm.e2e/foobar')
})
