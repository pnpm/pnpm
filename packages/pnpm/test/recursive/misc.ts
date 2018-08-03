import fs = require('mz/fs')
import isCI = require('is-ci')
import isWindows = require('is-windows')
import tape = require('tape')
import promisifyTape from 'tape-promise'
import path = require('path')
import writeJsonFile = require('write-json-file')
import writeYamlFile = require('write-yaml-file')
import {
  execPnpm,
  preparePackages,
  retryLoadJsonFile,
  spawn,
} from '../utils'
import mkdirp = require('mkdirp-promise')

const test = promisifyTape(tape)

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

  const modulesYaml1 = await projects['project-1'].loadModules()
  t.ok(modulesYaml1 && modulesYaml1.shamefullyFlatten)

  const modulesYaml2 = await projects['project-2'].loadModules()
  t.notOk(modulesYaml2 && modulesYaml2.shamefullyFlatten)
})

test('workspace .npmrc is always read', async (t: tape.Test) => {
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
  await fs.writeFile('.npmrc', 'shamefully-flatten = true', 'utf8')
  await fs.writeFile('project-2/.npmrc', 'shamefully-flatten = false', 'utf8')

  process.chdir('project-1')
  await execPnpm('install')

  t.ok(projects['project-1'].requireModule('is-positive'))

  const modulesYaml1 = await projects['project-1'].loadModules()
  t.ok(modulesYaml1 && modulesYaml1.shamefullyFlatten)

  process.chdir('..')
  process.chdir('project-2')

  await execPnpm('install')

  t.ok(projects['project-2'].requireModule('is-negative'))

  const modulesYaml2 = await projects['project-2'].loadModules()
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
  const server = spawn(['server', 'start'], {storeDir})

  const serverJsonPath = path.resolve(storeDir, '2', 'server', 'server.json')
  const serverJson = await retryLoadJsonFile(serverJsonPath)
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

  const shr1 = await projects['project-1'].loadShrinkwrap()
  t.ok(shr1.packages['/dep-of-pkg-with-1-dep/100.1.0'])

  const shr2 = await projects['project-2'].loadShrinkwrap()
  t.ok(shr2.packages['/dep-of-pkg-with-1-dep/100.1.0'])
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

  const shr1 = await projects['project-1'].loadShrinkwrap()
  t.notOk(shr1.packages['/dep-of-pkg-with-1-dep/100.1.0'])

  const shr2 = await projects['project-2'].loadShrinkwrap()
  t.notOk(shr2.packages['/dep-of-pkg-with-1-dep/100.1.0'])
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

  await writeYamlFile('pnpm-workspace.yaml', {packages: ['project-1']})

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

  await mkdirp('node_modules')
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

test('second run of `recursive link` after package.json has been edited manually', async t => {
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

  await execPnpm('recursive', 'link')

  await writeJsonFile('is-negative/package.json', {
    name: 'is-negative',
    version: '1.0.0',
    dependencies: {
      'is-positive': '1.0.0',
    },
  })

  await execPnpm('recursive', 'link')

  t.ok(projects['is-negative'].requireModule('is-positive/package.json'))
})

test('recursive --scope', async (t: tape.Test) => {
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

  await execPnpm('recursive', 'link', '--scope', 'project-1')

  projects['project-1'].has('is-positive')
  projects['project-2'].has('is-negative')
  projects['project-3'].hasNot('minimatch')
})

test('recursive --scope ignore excluded packages', async (t: tape.Test) => {
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

  await execPnpm('recursive', 'link', '--scope', 'project-1')

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

  await execPnpm('recursive', 'link', '--filter', 'project-1...')

  projects['project-1'].has('is-positive')
  projects['project-2'].has('is-negative')
  projects['project-3'].hasNot('minimatch')
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

  await execPnpm('recursive', 'link', '--filter', 'project-1', '--filter', 'project-2')

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

  await execPnpm('recursive', 'link', '--filter', 'project-1')

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
