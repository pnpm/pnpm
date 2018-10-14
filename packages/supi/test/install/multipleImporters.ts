import assertProject from '@pnpm/assert-project'
import { preparePackages } from '@pnpm/prepare'
import path = require('path')
import tape = require('tape')
import promisifyTape from 'tape-promise'
import { install, installPkgs } from 'supi'
import { testDefaults } from '../utils'

const test = promisifyTape(tape)
const testOnly = promisifyTape(tape.only)

test('install only the dependencies of the specified importer', async (t) => {
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
  ])

  const importers = [
    {
      prefix: path.resolve('project-1'),
    },
    {
      prefix: path.resolve('project-2'),
    },
  ]
  await install(await testDefaults({ importers, shrinkwrapOnly: true }))

  await install(await testDefaults({ importers: importers.slice(0, 1) }))

  await projects['project-1'].has('is-positive')
  await projects['project-2'].hasNot('is-negative')

  const rootNodeModules = assertProject(t, process.cwd())
  await rootNodeModules.has('.localhost+4873/is-positive/1.0.0')
  await rootNodeModules.hasNot('.localhost+4873/is-negative/1.0.0')
})

test('dependencies of other importers are not pruned when installing for a subset of importers', async (t) => {
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
  ])

  const importers = [
    {
      prefix: path.resolve('project-1'),
    },
    {
      prefix: path.resolve('project-2'),
    },
  ]
  await install(await testDefaults({ importers }))

  await installPkgs(['is-positive@2'], await testDefaults({ importers: importers.slice(0, 1) }))

  await projects['project-1'].has('is-positive')
  await projects['project-2'].has('is-negative')

  const rootNodeModules = assertProject(t, process.cwd())
  await rootNodeModules.has('.localhost+4873/is-positive/2.0.0')
  await rootNodeModules.hasNot('.localhost+4873/is-positive/1.0.0')
  await rootNodeModules.has('.localhost+4873/is-negative/1.0.0')
})

test('dependencies of other importers are not pruned when (headless) installing for a subset of importers', async (t) => {
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
  ])

  const importers = [
    {
      prefix: path.resolve('project-1'),
    },
    {
      prefix: path.resolve('project-2'),
    },
  ]
  await install(await testDefaults({ importers }))

  await installPkgs(['is-positive@2'], await testDefaults({ importers: importers.slice(0, 1), shrinkwrapOnly: true }))
  await install(await testDefaults({ importers: importers.slice(0, 1), frozenShrinkwrap: true }))

  await projects['project-1'].has('is-positive')
  await projects['project-2'].has('is-negative')

  const rootNodeModules = assertProject(t, process.cwd())
  await rootNodeModules.has('.localhost+4873/is-positive/2.0.0')
  await rootNodeModules.hasNot('.localhost+4873/is-positive/1.0.0')
  await rootNodeModules.has('.localhost+4873/is-negative/1.0.0')
})
