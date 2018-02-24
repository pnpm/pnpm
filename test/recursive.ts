import delay = require('delay')
import fs = require('mz/fs')
import isCI = require('is-ci')
import isWindows = require('is-windows')
import tape = require('tape')
import promisifyTape from 'tape-promise'
import path = require('path')
import exists = require('path-exists')
import writeYamlFile = require('write-yaml-file')
import {
  execPnpm,
  prepare,
  retryLoadJsonFile,
  spawn,
} from './utils'

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

  t.end()
})

test('recursive installation using server', async t => {
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

  const server = spawn(['server', 'start'])

  const serverJsonPath = path.resolve('..', 'store', '2', 'server.json')
  const serverJson = await retryLoadJsonFile(serverJsonPath)
  t.ok(serverJson)
  t.ok(serverJson.connectionOptions)

  await execPnpm('recursive', 'install')

  t.ok(projects['project-1'].requireModule('is-positive'))
  t.ok(projects['project-2'].requireModule('is-negative'))

  await execPnpm('server', 'stop')

  t.end()
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

  t.end()
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

  t.end()
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

  await execPnpm('recursive', 'dislink')

  process.chdir('project-1')
  t.ok(await exists('node_modules', 'is-positive', 'index.js'), 'local package is dislinked')

  t.end()
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

  t.end()
})
