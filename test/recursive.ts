import delay = require('delay')
import fs = require('mz/fs')
import isCI = require('is-ci')
import isWindows = require('is-windows')
import tape = require('tape')
import promisifyTape from 'tape-promise'
import thenify = require('thenify')
import path = require('path')
import {
  prepare,
  execPnpm,
  spawn,
  retryLoadJsonFile,
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
