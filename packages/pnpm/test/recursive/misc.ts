import { preparePackages } from '@pnpm/prepare'
import isCI = require('is-ci')
import isWindows = require('is-windows')
import makeDir = require('make-dir')
import fs = require('mz/fs')
import path = require('path')
import readYamlFile from 'read-yaml-file'
import tape = require('tape')
import promisifyTape from 'tape-promise'
import writeJsonFile = require('write-json-file')
import writeYamlFile = require('write-yaml-file')
import {
  execPnpm,
  retryLoadJsonFile,
  spawn,
} from '../utils'

const test = promisifyTape(tape)
const testOnly = promisifyTape(tape.only)

test('recursive install/uninstall', async (t: tape.Test) => {
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

  await execPnpm('recursive', 'install')

  t.ok(projects['project-1'].requireModule('is-positive'))
  t.ok(projects['project-2'].requireModule('is-negative'))
  await projects['project-2'].has('is-negative')

  await execPnpm('recursive', 'install', 'noop')

  t.ok(projects['project-1'].requireModule('noop'))
  t.ok(projects['project-2'].requireModule('noop'))

  await execPnpm('recursive', 'uninstall', 'is-negative')

  await projects['project-2'].hasNot('is-negative')
})

test('recursive install using "install -r"', async (t: tape.Test) => {
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

  await execPnpm('install', '-r')

  t.ok(projects['project-1'].requireModule('is-positive'))
  t.ok(projects['project-2'].requireModule('is-negative'))
})

test('recursive install using "install --recursive"', async (t: tape.Test) => {
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

  await execPnpm('install', '--recursive')

  t.ok(projects['project-1'].requireModule('is-positive'))
  t.ok(projects['project-2'].requireModule('is-negative'))
})

test('recursive install should install in whole workspace even when executed in a subdirectory', async (t: tape.Test) => {
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

  await fs.writeFile('pnpm-workspace.yaml', '', 'utf8')

  process.chdir('project-1')

  await execPnpm('recursive', 'install')

  t.ok(projects['project-1'].requireModule('is-positive'))
  t.ok(projects['project-2'].requireModule('is-negative'))
})

// Created to cover the issue described in https://github.com/pnpm/pnpm/issues/1253
test('recursive install with package that has link', async (t: tape.Test) => {
  const projects = preparePackages(t, [
    {
      name: 'project-1',
      version: '1.0.0',

      dependencies: {
        'is-positive': '1.0.0',
        'project-2': 'link:../project-2',
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

  await execPnpm('recursive', 'install')

  t.ok(projects['project-1'].requireModule('is-positive'))
  t.ok(projects['project-1'].requireModule('project-2/package.json'))
  t.ok(projects['project-2'].requireModule('is-negative'))
})

test('recursive update', async (t: tape.Test) => {
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

  await execPnpm('recursive', 'install')

  await execPnpm('recursive', 'update', 'is-positive@2.0.0')

  t.equal(projects['project-1'].requireModule('is-positive/package.json').version, '2.0.0')
  projects['project-2'].hasNot('is-positive')
})

test('recursive installation with package-specific .npmrc', async t => {
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

  await fs.writeFile('project-1/.npmrc', 'shamefully-flatten = true', 'utf8')

  await execPnpm('recursive', 'install')

  t.ok(projects['project-1'].requireModule('is-positive'))
  t.ok(projects['project-2'].requireModule('is-negative'))

  const modulesYaml1 = await readYamlFile<{ shamefullyFlatten: boolean }>(path.resolve('project-1', 'node_modules', '.modules.yaml'))
  t.ok(modulesYaml1 && modulesYaml1.shamefullyFlatten)

  const modulesYaml2 = await readYamlFile<{ shamefullyFlatten: boolean }>(path.resolve('project-2', 'node_modules', '.modules.yaml'))
  t.notOk(modulesYaml2 && modulesYaml2.shamefullyFlatten)
})

test('workspace .npmrc is always read', async (t: tape.Test) => {
  const projects = preparePackages(t, [
    {
      location: 'workspace/project-1',
      package: {
        name: 'project-1',
        version: '1.0.0',

        dependencies: {
          'is-positive': '1.0.0',
        },
      },
    },
    {
      location: 'workspace/project-2',
      package: {
        name: 'project-2',
        version: '1.0.0',

        dependencies: {
          'is-negative': '1.0.0',
        },
      },
    },
  ])

  const storeDir = path.resolve('../store')
  await fs.writeFile('pnpm-workspace.yaml', '', 'utf8')
  await fs.writeFile('.npmrc', 'shamefully-flatten = true\nshared-workspace-lockfile=false', 'utf8')
  await fs.writeFile('project-2/.npmrc', 'shamefully-flatten = false', 'utf8')

  process.chdir('project-1')
  await execPnpm('install', '--store', storeDir)

  t.ok(projects['project-1'].requireModule('is-positive'))

  const modulesYaml1 = await readYamlFile<{ shamefullyFlatten: boolean }>(path.resolve('node_modules', '.modules.yaml'))
  t.ok(modulesYaml1 && modulesYaml1.shamefullyFlatten)

  process.chdir('..')
  process.chdir('project-2')

  await execPnpm('install', '--store', storeDir)

  t.ok(projects['project-2'].requireModule('is-negative'))

  const modulesYaml2 = await readYamlFile<{ shamefullyFlatten: boolean }>(path.resolve('node_modules', '.modules.yaml'))
  t.ok(modulesYaml2 && modulesYaml2.shamefullyFlatten === false)
})

test('recursive installation using server', async (t: tape.Test) => {
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

  const storeDir = path.resolve('store')
  const server = spawn(['server', 'start'], { storeDir })

  const serverJsonPath = path.resolve(storeDir, '2', 'server', 'server.json')
  const serverJson = await retryLoadJsonFile<{ connectionOptions: object }>(serverJsonPath)
  t.ok(serverJson)
  t.ok(serverJson.connectionOptions)

  await execPnpm('recursive', 'install')

  t.ok(projects['project-1'].requireModule('is-positive'))
  t.ok(projects['project-2'].requireModule('is-negative'))

  await execPnpm('server', 'stop', '--store', storeDir)
})

test('recursive installation of packages with hooks', async t => {
  // This test hangs on Appveyor for some reason
  if (isCI && isWindows()) return
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

  process.chdir('project-1')
  const pnpmfile = `
    module.exports = { hooks: { readPackage } }
    function readPackage (pkg) {
      pkg.dependencies = pkg.dependencies || {}
      pkg.dependencies['dep-of-pkg-with-1-dep'] = '100.1.0'
      return pkg
    }
  `
  await fs.writeFile('pnpmfile.js', pnpmfile, 'utf8')

  process.chdir('../project-2')
  await fs.writeFile('pnpmfile.js', pnpmfile, 'utf8')

  process.chdir('..')

  await execPnpm('recursive', 'install')

  const lockfile1 = await projects['project-1'].readLockfile()
  t.ok(lockfile1.packages['/dep-of-pkg-with-1-dep/100.1.0'])

  const lockfile2 = await projects['project-2'].readLockfile()
  t.ok(lockfile2.packages['/dep-of-pkg-with-1-dep/100.1.0'])
})

test('ignores pnpmfile.js during recursive installation when --ignore-pnpmfile is used', async t => {
  // This test hangs on Appveyor for some reason
  if (isCI && isWindows()) return
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

  process.chdir('project-1')
  const pnpmfile = `
    module.exports = { hooks: { readPackage } }
    function readPackage (pkg) {
      pkg.dependencies = pkg.dependencies || {}
      pkg.dependencies['dep-of-pkg-with-1-dep'] = '100.1.0'
      return pkg
    }
  `
  await fs.writeFile('pnpmfile.js', pnpmfile, 'utf8')

  process.chdir('../project-2')
  await fs.writeFile('pnpmfile.js', pnpmfile, 'utf8')

  process.chdir('..')

  await execPnpm('recursive', 'install', '--ignore-pnpmfile')

  const lockfile1 = await projects['project-1'].readLockfile()
  t.notOk(lockfile1.packages['/dep-of-pkg-with-1-dep/100.1.0'])

  const lockfile2 = await projects['project-2'].readLockfile()
  t.notOk(lockfile2.packages['/dep-of-pkg-with-1-dep/100.1.0'])
})

test('running `pnpm recursive` on a subset of packages', async t => {
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

  await writeYamlFile('pnpm-workspace.yaml', { packages: ['project-1'] })

  await execPnpm('recursive', 'install')

  await projects['project-1'].has('is-positive')
  await projects['project-2'].hasNot('is-negative')
})

test('running `pnpm recursive` only for packages in subdirectories of cwd', async t => {
  const projects = preparePackages(t, [
    {
      location: 'packages/project-1',
      package: {
        name: 'project-1',
        version: '1.0.0',

        dependencies: {
          'is-positive': '1.0.0',
        },
      },
    },
    {
      location: 'packages/project-2',
      package: {
        name: 'project-2',
        version: '1.0.0',

        dependencies: {
          'is-negative': '1.0.0',
        },
      }
    },
    {
      location: 'root-project',
      package: {
        name: 'root-project',
        version: '1.0.0',

        dependencies: {
          'debug': '*',
        },
      }
    }
  ])

  await makeDir('node_modules')
  process.chdir('packages')

  await execPnpm('recursive', 'install')

  await projects['project-1'].has('is-positive')
  await projects['project-2'].has('is-negative')
  await projects['root-project'].hasNot('debug')
})

test('recursive installation fails when installation in one of the packages fails', async t => {
  const projects = preparePackages(t, [
    {
      name: 'project-1',
      version: '1.0.0',

      dependencies: {
        'this-pkg-does-not-exist': '100.100.100',
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

  try {
    await execPnpm('recursive', 'install')
    t.fail('The command should have failed')
  } catch (err) {
    t.ok(err, 'the command failed')
  }
})

test('second run of `recursive install` after package.json has been edited manually', async t => {
  const projects = preparePackages(t, [
    {
      name: 'is-negative',
      version: '1.0.0',

      dependencies: {
        'is-positive': '2.0.0',
      },
    },
    {
      name: 'is-positive',
      version: '1.0.0',
    },
  ])

  await execPnpm('recursive', 'install')

  await writeJsonFile('is-negative/package.json', {
    name: 'is-negative',
    version: '1.0.0',

    dependencies: {
      'is-positive': '1.0.0',
    },
  })

  await execPnpm('recursive', 'install')

  t.ok(projects['is-negative'].requireModule('is-positive/package.json'))
})

test('recursive --filter', async (t: tape.Test) => {
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
        minimatch: '*',
      },
    },
  ])

  await execPnpm('recursive', 'install', '--filter', 'project-1...')

  projects['project-1'].has('is-positive')
  projects['project-2'].has('is-negative')
  projects['project-3'].hasNot('minimatch')
})

test('recursive --filter ignore excluded packages', async (t: tape.Test) => {
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
        minimatch: '*',
      },
    },
  ])

  await writeYamlFile('pnpm-workspace.yaml', {
    packages: [
      '**',
      '!project-1'
    ],
  })

  await execPnpm('recursive', 'install', '--filter', 'project-1...')

  projects['project-1'].hasNot('is-positive')
  projects['project-2'].hasNot('is-negative')
  projects['project-3'].hasNot('minimatch')
})

test('recursive filter package with dependencies', async (t: tape.Test) => {
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
        minimatch: '*',
      },
    },
  ])

  await execPnpm('recursive', 'install', '--filter', 'project-1...')

  projects['project-1'].has('is-positive')
  projects['project-2'].has('is-negative')
  projects['project-3'].hasNot('minimatch')
})

test('recursive filter package with dependents', async (t: tape.Test) => {
  const projects = preparePackages(t, [
    {
      name: 'project-0',
      version: '1.0.0',

      dependencies: {
        'is-positive': '1.0.0',
        'project-1': '1.0.0',
      },
    },
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
        minimatch: '*',
      },
    },
  ])

  await execPnpm('recursive', 'install', '--filter', '...project-2')

  projects['project-0'].has('is-positive')
  projects['project-1'].has('is-positive')
  projects['project-2'].has('is-negative')
  projects['project-3'].hasNot('minimatch')
})

test('recursive filter package with dependents and filter with dependencies', async (t: tape.Test) => {
  const projects = preparePackages(t, [
    {
      name: 'project-0',
      version: '1.0.0',

      dependencies: {
        'is-positive': '1.0.0',
        'project-1': '1.0.0',
      },
    },
    {
      name: 'project-1',
      version: '1.0.0',

      dependencies: {
        'is-positive': '1.0.0',
        'project-2': '1.0.0',
        'project-3': '1.0.0',
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
        minimatch: '*',
      },
    },
    {
      name: 'project-4',
      version: '1.0.0',

      dependencies: {
        minimatch: '*',
      },
    },
  ])

  await execPnpm('recursive', 'install', '--filter', '...project-2', '--filter', 'project-1...')

  projects['project-0'].has('is-positive')
  projects['project-1'].has('is-positive')
  projects['project-2'].has('is-negative')
  projects['project-3'].has('minimatch')
  projects['project-4'].hasNot('minimatch')
})

test('recursive filter package with dependents and filter with dependencies, using dash-dash', async (t: tape.Test) => {
  const projects = preparePackages(t, [
    {
      name: 'project-0',
      version: '1.0.0',

      dependencies: {
        'is-positive': '1.0.0',
        'project-1': '1.0.0',
      },
    },
    {
      name: 'project-1',
      version: '1.0.0',

      dependencies: {
        'is-positive': '1.0.0',
        'project-2': '1.0.0',
        'project-3': '1.0.0',
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
        minimatch: '*',
      },
    },
    {
      name: 'project-4',
      version: '1.0.0',

      dependencies: {
        minimatch: '*',
      },
    },
  ])

  await execPnpm('recursive', 'install', '--', '...project-2', 'project-1...')

  projects['project-0'].has('is-positive')
  projects['project-1'].has('is-positive')
  projects['project-2'].has('is-negative')
  projects['project-3'].has('minimatch')
  projects['project-4'].hasNot('minimatch')
})

test('recursive filter package with dependents and filter with dependencies, using dash-dash, omitting the recursive command', async (t: tape.Test) => {
  const projects = preparePackages(t, [
    {
      name: 'project-0',
      version: '1.0.0',

      dependencies: {
        'is-positive': '1.0.0',
        'project-1': '1.0.0',
      },
    },
    {
      name: 'project-1',
      version: '1.0.0',

      dependencies: {
        'is-positive': '1.0.0',
        'project-2': '1.0.0',
        'project-3': '1.0.0',
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
        minimatch: '*',
      },
    },
    {
      name: 'project-4',
      version: '1.0.0',

      dependencies: {
        minimatch: '*',
      },
    },
  ])

  await execPnpm('--link-workspace-packages', 'install', '--', '...project-2', 'project-1...')

  projects['project-0'].has('is-positive')
  projects['project-1'].has('is-positive')
  projects['project-2'].has('is-negative')
  projects['project-3'].has('minimatch')
  projects['project-4'].hasNot('minimatch')
})

test('recursive filter multiple times', async (t: tape.Test) => {
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
        minimatch: '*',
      },
    },
  ])

  await execPnpm('recursive', 'install', '--filter', 'project-1', '--filter', 'project-2')

  projects['project-1'].has('is-positive')
  projects['project-2'].has('is-negative')
  projects['project-3'].hasNot('minimatch')
})

test('recursive filter package without dependencies', async (t: tape.Test) => {
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
        minimatch: '*',
      },
    },
  ])

  await execPnpm('recursive', 'install', '--filter', 'project-1')

  projects['project-1'].has('is-positive')
  projects['project-2'].hasNot('is-negative')
  projects['project-3'].hasNot('minimatch')
})

test('recursive filter by location', async (t: tape.Test) => {
  const projects = preparePackages(t, [
    {
      location: 'packages/project-1',
      package: {
        name: 'project-1',
        version: '1.0.0',

        dependencies: {
          'is-positive': '1.0.0',
          'project-2': '1.0.0',
        },
      },
    },
    {
      location: 'packages/project-2',
      package: {
        name: 'project-2',
        version: '1.0.0',

        dependencies: {
          'is-negative': '1.0.0',
        },
      },
    },
    {
      name: 'project-3',
      version: '1.0.0',

      dependencies: {
        minimatch: '*',
      },
    },
  ])

  await execPnpm('recursive', 'install', '--filter', './packages')

  projects['project-1'].has('is-positive')
  projects['project-2'].has('is-negative')
  projects['project-3'].hasNot('minimatch')
})

test('recursive filter by location is relative to current working directory', async (t: tape.Test) => {
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

  await fs.writeFile('pnpm-workspace.yaml', '', 'utf8')

  process.chdir('project-1')

  await execPnpm('recursive', 'install', '--filter', '.')

  t.ok(projects['project-1'].requireModule('is-positive'))
  await projects['project-2'].hasNot('is-negative')
})

test('recursive command with filter from config', async (t: tape.Test) => {
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
        minimatch: '*',
      },
    },
  ])

  await fs.writeFile('.npmrc', 'filter=project-1 project-2', 'utf8')
  await execPnpm('recursive', 'install')

  projects['project-1'].has('is-positive')
  projects['project-2'].has('is-negative')
  projects['project-3'].hasNot('minimatch')
})

test('non-recursive install ignores filter from config', async (t: tape.Test) => {
  const projects = preparePackages(t, [
    {
      location: '.',
      package: {
        name: 'project-1',
        version: '1.0.0',

        dependencies: {
          'is-positive': '1.0.0',
        },
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
        minimatch: '*',
      },
    },
  ])

  await fs.writeFile('.npmrc', 'filter=project-2', 'utf8')
  await execPnpm('install')

  projects['project-1'].has('is-positive')
  projects['project-2'].hasNot('is-negative')
  projects['project-3'].hasNot('minimatch')
})

test('recursive install --no-bail', async (t: tape.Test) => {
  const projects = preparePackages(t, [
    {
      name: 'project-1',
      version: '1.0.0',

      dependencies: {
        '@pnpm/this-does-not-exist': '1.0.0',
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

  let failed = false

  try {
    await execPnpm('recursive', 'install', '--no-bail')
  } catch (err) {
    failed = true
  }

  t.ok(failed, 'command failed')

  t.ok(projects['project-2'].requireModule('is-negative'))
})
