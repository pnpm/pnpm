import prepare, { preparePackages } from '@pnpm/prepare'
import fs = require('mz/fs')
import tape = require('tape')
import promisifyTape from 'tape-promise'
import writeYamlFile = require('write-yaml-file')
import { execPnpm } from '../utils'

const test = promisifyTape(tape)
const testOnly = promisifyTape(tape.only)

test('hoist the dependency graph', async function (t) {
  const project = prepare(t)

  await execPnpm('install', '--shamefully-flatten', 'express@4.16.2')

  await project.has('express')
  await project.has('.pnpm/node_modules/debug')
  await project.has('.pnpm/node_modules/cookie')

  await execPnpm('uninstall', '--shamefully-flatten', 'express')

  await project.hasNot('express')
  await project.hasNot('.pnpm/node_modules/debug')
  await project.hasNot('.pnpm/node_modules/cookie')
})

test('hoist-pattern: applied to all the workspace packages when set to true in the root .npmrc file', async (t: tape.Test) => {
  const projects = preparePackages(t, [
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
        'foobar': '100.0.0',
      },
    },
  ])

  await writeYamlFile('pnpm-workspace.yaml', { packages: ['**', '!store/**'] })
  await fs.writeFile('.npmrc', 'shamefully-flatten', 'utf8')

  await execPnpm('recursive', 'install')

  await projects['root'].has('.pnpm/node_modules/dep-of-pkg-with-1-dep')
  await projects['root'].has('.pnpm/node_modules/foo')
  await projects['project'].hasNot('foo')
  await projects['project'].has('foobar')
})
