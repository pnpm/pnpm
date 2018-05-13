import delay = require('delay')
import fs = require('mz/fs')
import isCI = require('is-ci')
import isWindows = require('is-windows')
import tape = require('tape')
import promisifyTape from 'tape-promise'
import path = require('path')
import exists = require('path-exists')
import writeJsonFile = require('write-json-file')
import writeYamlFile = require('write-yaml-file')
import {
  execPnpm,
  prepare,
  retryLoadJsonFile,
  spawn,
} from './utils'
import mkdirp = require('mkdirp-promise')
import loadYamlFile = require('load-yaml-file')

const test = promisifyTape(tape)

test('recursive installation', async t => {
  const projects = prepare(t, [
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
})

test('recursive installation with package-specific .npmrc', async t => {
  const projects = prepare(t, [
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
  t.ok(modulesYaml1.shamefullyFlatten)

  const modulesYaml2 = await projects['project-2'].loadModules()
  t.notOk(modulesYaml2.shamefullyFlatten)
})

test('recursive installation using server', async (t: tape.Test) => {
  const projects = prepare(t, [
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
  const projects = prepare(t, [
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
  const projects = prepare(t, [
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

test('recursive linking/unlinking', async t => {
  const projects = prepare(t, [
    {
      name: 'project-1',
      version: '1.0.0',
      dependencies: {
        'is-positive': '1.0.0',
      },
    },
    {
      name: 'is-positive',
      version: '1.0.0',
      dependencies: {
        'is-negative': '1.0.0',
      },
    },
  ])

  await execPnpm('recursive', 'link')

  t.ok(projects['is-positive'].requireModule('is-negative'))
  t.notOk(projects['project-1'].requireModule('is-positive/package.json').author, 'local package is linked')

  await execPnpm('recursive', 'unlink')

  process.chdir('project-1')
  t.ok(await exists('node_modules', 'is-positive', 'index.js'), 'local package is unlinked')

  const project1Shr = await projects['project-1'].loadShrinkwrap()
  t.equal(project1Shr.registry, 'http://localhost:4873/', 'project-1 has correct registry specified in shrinkwrap.yaml')

  const isPositiveShr = await projects['is-positive'].loadShrinkwrap()
  t.equal(isPositiveShr.registry, 'http://localhost:4873/', 'is-positive has correct registry specified in shrinkwrap.yaml')
})

test('running `pnpm recursive` on a subset of packages', async t => {
  const projects = prepare(t, [
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
  const projects = prepare(t, [
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
  const projects = prepare(t, [
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
  const projects = prepare(t, [
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
