import { Lockfile } from '@pnpm/lockfile-types'
import prepare, { preparePackages } from '@pnpm/prepare'
import isCI = require('is-ci')
import isWindows = require('is-windows')
import fs = require('mz/fs')
import path = require('path')
import readYamlFile from 'read-yaml-file'
import tape = require('tape')
import promisifyTape from 'tape-promise'
import writeYamlFile = require('write-yaml-file')
import {
  execPnpm,
  execPnpmSync,
  retryLoadJsonFile,
  spawnPnpm,
} from '../utils'

const test = promisifyTape(tape)
const testOnly = promisifyTape(tape.only)

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

  await fs.writeFile('project-2/.npmrc', 'hoist = false', 'utf8')

  await execPnpm('recursive', 'install')

  t.ok(projects['project-1'].requireModule('is-positive'))
  t.ok(projects['project-2'].requireModule('is-negative'))

  const modulesYaml1 = await readYamlFile<{ hoistPattern: string }>(path.resolve('project-1', 'node_modules', '.modules.yaml'))
  t.deepEqual(modulesYaml1 && modulesYaml1.hoistPattern, ['*'])

  const modulesYaml2 = await readYamlFile<{ hoistPattern: string }>(path.resolve('project-2', 'node_modules', '.modules.yaml'))
  t.notOk(modulesYaml2 && modulesYaml2.hoistPattern)
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
  await fs.writeFile('project-2/.npmrc', 'hoist=false', 'utf8')

  process.chdir('project-1')
  await execPnpm('install', '--store-dir', storeDir, '--filter', '.')

  t.ok(projects['project-1'].requireModule('is-positive'))

  const modulesYaml1 = await readYamlFile<{ hoistPattern: string }>(path.resolve('node_modules', '.modules.yaml'))
  t.deepEqual(modulesYaml1 && modulesYaml1.hoistPattern, ['*'])

  process.chdir('..')
  process.chdir('project-2')

  await execPnpm('install', '--store-dir', storeDir, '--filter', '.')

  t.ok(projects['project-2'].requireModule('is-negative'))

  const modulesYaml2 = await readYamlFile<{ hoistPattern: string }>(path.resolve('node_modules', '.modules.yaml'))
  t.notOk(modulesYaml2 && modulesYaml2.hoistPattern)
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
  const server = spawnPnpm(['server', 'start'], { storeDir })

  const serverJsonPath = path.resolve(storeDir, '2', 'server', 'server.json')
  const serverJson = await retryLoadJsonFile<{ connectionOptions: object }>(serverJsonPath)
  t.ok(serverJson)
  t.ok(serverJson.connectionOptions)

  await execPnpm('recursive', 'install')

  t.ok(projects['project-1'].requireModule('is-positive'))
  t.ok(projects['project-2'].requireModule('is-negative'))

  await execPnpm('server', 'stop', '--store-dir', storeDir)
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

test('recursive installation of packages in workspace ignores hooks in packages', async t => {
  // This test hangs on Appveyor for some reason
  if (isCI && isWindows()) return
  preparePackages(t, [
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
  await fs.writeFile('pnpmfile.js', `
    module.exports = { hooks: { readPackage } }
    function readPackage (pkg) {
      pkg.dependencies = pkg.dependencies || {}
      pkg.dependencies['is-number'] = '1.0.0'
      return pkg
    }
  `)

  await writeYamlFile('pnpm-workspace.yaml', { packages: ['project-1', 'project-2'] })

  await execPnpm('recursive', 'install')

  const lockfile = await readYamlFile<Lockfile>('pnpm-lock.yaml')
  // tslint:disable: no-unnecessary-type-assertion
  t.notOk(lockfile.packages!['/dep-of-pkg-with-1-dep/100.1.0'])
  t.ok(lockfile.packages!['/is-number/1.0.0'])
  // tslint:enable: no-unnecessary-type-assertion
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

test('adding new dependency in the root should fail if --ignore-workspace-root-check is not used', async (t: tape.Test) => {
  const project = prepare(t)

  await fs.writeFile('pnpm-workspace.yaml', '', 'utf8')

  {
    const { status, stderr } = execPnpmSync('add', 'is-positive')

    t.equal(status, 1)

    t.ok(
      stderr.toString().includes(
        'Running this command will add the dependency to the workspace root, ' +
        'which might not be what you want - if you really meant it, ' +
        'make it explicit by running this command again with the -W flag (or --ignore-workspace-root-check).'
      )
    )
  }

  {
    const { status } = execPnpmSync('add', 'is-positive', '--ignore-workspace-root-check')

    t.equal(status, 0)
    await project.has('is-positive')
  }

  {
    const { status } = execPnpmSync('add', 'is-negative', '-W')

    t.equal(status, 0)
    await project.has('is-negative')
  }
})
