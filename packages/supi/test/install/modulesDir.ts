import { prepareEmpty, preparePackages } from '@pnpm/prepare'
import {
  install,
  MutatedProject,
  mutateModules,
} from 'supi'
import promisifyTape from 'tape-promise'
import { testDefaults } from '../utils'
import path = require('path')
import rimraf = require('@zkochan/rimraf')
import tape = require('tape')

const test = promisifyTape(tape)

test('installing to a custom modules directory', async (t: tape.Test) => {
  const project = prepareEmpty(t)

  await install({
    dependencies: {
      'is-positive': '1.0.0',
    },
  }, await testDefaults({ modulesDir: 'pnpm_modules' }))

  await project.has('is-positive', 'pnpm_modules')

  await rimraf('pnpm_modules')
  await project.hasNot('is-positive', 'pnpm_modules')

  await install({
    dependencies: {
      'is-positive': '1.0.0',
    },
  }, await testDefaults({ frozenLockfile: true, modulesDir: 'pnpm_modules' }))

  await project.has('is-positive', 'pnpm_modules')
})

test('using different custom modules directory for every project', async (t) => {
  const projects = preparePackages(t, [
    {
      location: 'project-1',
      package: {
        name: 'project-1',

        dependencies: { 'is-positive': '1.0.0' },
      },
    },
    {
      location: 'project-2',
      package: {
        name: 'project-2',

        dependencies: { 'is-positive': '1.0.0' },
      },
    },
  ])

  const importers: MutatedProject[] = [
    {
      buildIndex: 0,
      manifest: {
        name: 'project-1',
        version: '1.0.0',

        dependencies: {
          'is-positive': '1.0.0',
        },
      },
      modulesDir: 'modules_1',
      mutation: 'install',
      rootDir: path.resolve('project-1'),
    },
    {
      buildIndex: 0,
      manifest: {
        name: 'project-2',
        version: '1.0.0',

        dependencies: {
          'is-positive': '1.0.0',
        },
      },
      modulesDir: 'modules_2',
      mutation: 'install',
      rootDir: path.resolve('project-2'),
    },
  ]
  await mutateModules(importers, await testDefaults())

  await projects['project-1'].has('is-positive', 'modules_1')
  await projects['project-2'].has('is-positive', 'modules_2')
})
