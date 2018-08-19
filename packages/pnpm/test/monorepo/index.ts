import fs = require('mz/fs')
import tape = require('tape')
import promisifyTape from 'tape-promise'
import path = require('path')
import writeYamlFile = require('write-yaml-file')
import {
  preparePackages,
  execPnpm,
 } from '../utils'

const test = promisifyTape(tape)

test('linking a package inside a monorepo', async (t: tape.Test) => {
  const projects = preparePackages(t, [
    {
      name: 'project-1',
      version: '1.0.0',
    },
    {
      name: 'project-2',
      version: '2.0.0',
    },
    {
      name: 'project-3',
      version: '3.0.0',
    },
    {
      name: 'project-4',
      version: '4.0.0',
    },
  ])

  await writeYamlFile('pnpm-workspace.yaml', {packages: ['**', '!store/**']})

  process.chdir('project-1')

  await execPnpm('link', 'project-2')

  await execPnpm('link', 'project-3', '--save-dev')

  await execPnpm('link', 'project-4', '--save-optional')

  const pkg = await import(path.resolve('package.json'))

  t.deepEqual(pkg && pkg.dependencies, {'project-2': '^2.0.0'}, 'spec of linked package added to dependencies')
  t.deepEqual(pkg && pkg.devDependencies, {'project-3': '^3.0.0'}, 'spec of linked package added to devDependencies')
  t.deepEqual(pkg && pkg.optionalDependencies, {'project-4': '^4.0.0'}, 'spec of linked package added to optionalDependencies')

  await projects['project-1'].has('project-2')
  await projects['project-1'].has('project-3')
  await projects['project-1'].has('project-4')
})

test('linking a package inside a monorepo with --link-workspace-packages when installing new dependencies', async (t: tape.Test) => {
  const projects = preparePackages(t, [
    {
      name: 'project-1',
      version: '1.0.0',
    },
    {
      name: 'project-2',
      version: '2.0.0',
    },
    {
      name: 'project-3',
      version: '3.0.0',
    },
    {
      name: 'project-4',
      version: '4.0.0',
    },
  ])

  await fs.writeFile('.npmrc', 'link-workspace-packages = true', 'utf8')
  await writeYamlFile('pnpm-workspace.yaml', {packages: ['**', '!store/**']})

  process.chdir('project-1')

  await execPnpm('install', 'project-2')

  await execPnpm('install', 'project-3', '--save-dev')

  await execPnpm('install', 'project-4', '--save-optional')

  const pkg = await import(path.resolve('package.json'))

  t.deepEqual(pkg && pkg.dependencies, {'project-2': '^2.0.0'}, 'spec of linked package added to dependencies')
  t.deepEqual(pkg && pkg.devDependencies, {'project-3': '^3.0.0'}, 'spec of linked package added to devDependencies')
  t.deepEqual(pkg && pkg.optionalDependencies, {'project-4': '^4.0.0'}, 'spec of linked package added to optionalDependencies')

  await projects['project-1'].has('project-2')
  await projects['project-1'].has('project-3')
  await projects['project-1'].has('project-4')
})

test('linking a package inside a monorepo with --link-workspace-packages', async (t: tape.Test) => {
  const projects = preparePackages(t, [
    {
      name: 'project-1',
      version: '1.0.0',
      dependencies: {
        'json-append': '1',
        'project-2': '2.0.0',
      },
      devDependencies: {
        'project-3': '3.0.0',
      },
      optionalDependencies: {
        'project-4': '4.0.0',
      },
      scripts: {
        install: `node -e "process.stdout.write('project-1')" | json-append ../output.json`,
      },
    },
    {
      name: 'project-2',
      version: '2.0.0',
      dependencies: {
        'json-append': '1',
      },
      scripts: {
        install: `node -e "process.stdout.write('project-2')" | json-append ../output.json`,
      },
    },
    {
      name: 'project-3',
      version: '3.0.0',
    },
    {
      name: 'project-4',
      version: '4.0.0',
    },
  ])

  await fs.writeFile('.npmrc', 'link-workspace-packages = true', 'utf8')
  await writeYamlFile('pnpm-workspace.yaml', {packages: ['**', '!store/**']})

  process.chdir('project-1')

  await execPnpm('install')

  const outputs = await import(path.resolve('..', 'output.json')) as string[]
  t.deepEqual(outputs, ['project-2', 'project-1'])

  await projects['project-1'].has('project-2')
  await projects['project-1'].has('project-3')
  await projects['project-1'].has('project-4')

  const shr = await projects['project-1'].loadShrinkwrap()
  t.equal(shr.dependencies['project-2'], 'link:../project-2')
  t.equal(shr.devDependencies['project-3'], 'link:../project-3')
  t.equal(shr.optionalDependencies['project-4'], 'link:../project-4')
})
