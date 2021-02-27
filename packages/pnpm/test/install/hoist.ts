import { promises as fs } from 'fs'
import prepare, { preparePackages } from '@pnpm/prepare'
import writeYamlFile from 'write-yaml-file'
import { execPnpm } from '../utils'

test('hoist the dependency graph', async () => {
  const project = prepare()

  await execPnpm(['install', 'express@4.16.2'])

  await project.has('express')
  await project.has('.pnpm/node_modules/debug')
  await project.has('.pnpm/node_modules/cookie')

  await execPnpm(['uninstall', 'express'])

  await project.hasNot('express')
  await project.hasNot('.pnpm/node_modules/debug')
  await project.hasNot('.pnpm/node_modules/cookie')
})

test('shamefully hoist the dependency graph', async () => {
  const project = prepare()

  await execPnpm(['add', '--shamefully-hoist', 'express@4.16.2'])

  await project.has('express')
  await project.has('debug')
  await project.has('cookie')

  await execPnpm(['remove', 'express'])

  await project.hasNot('express')
  await project.hasNot('debug')
  await project.hasNot('cookie')
})

test('shamefully-hoist: applied to all the workspace projects when set to true in the root .npmrc file', async () => {
  const projects = preparePackages([
    {
      location: '.',
      package: {
        name: 'root',

        dependencies: {
          'pkg-with-1-dep': '100.0.0',
        },
      },
    },
    {
      name: 'project',
      version: '1.0.0',

      dependencies: {
        foobar: '100.0.0',
      },
    },
  ])

  await writeYamlFile('pnpm-workspace.yaml', { packages: ['**', '!store/**'] })
  await fs.writeFile('.npmrc', 'shamefully-hoist', 'utf8')

  await execPnpm(['recursive', 'install'])

  await projects.root.has('dep-of-pkg-with-1-dep')
  await projects.root.has('foo')
  await projects.project.hasNot('foo')
  await projects.project.has('foobar')
})
