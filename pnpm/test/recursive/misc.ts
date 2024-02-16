import fs from 'fs'
import path from 'path'
import { prepare, preparePackages } from '@pnpm/prepare'
import { type Lockfile } from '@pnpm/lockfile-types'
import { sync as readYamlFile } from 'read-yaml-file'
import { isCI } from 'ci-info'
import isWindows from 'is-windows'
import { sync as writeYamlFile } from 'write-yaml-file'
import {
  execPnpm,
  execPnpmSync,
  retryLoadJsonFile,
  spawnPnpm,
} from '../utils'

const skipOnWindows = isWindows() ? test.skip : test

test('recursive installation with package-specific .npmrc', async () => {
  const projects = preparePackages([
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

  fs.writeFileSync('project-2/.npmrc', 'hoist = false', 'utf8')

  await execPnpm(['recursive', 'install'])

  expect(projects['project-1'].requireModule('is-positive')).toBeTruthy()
  expect(projects['project-2'].requireModule('is-negative')).toBeTruthy()

  const modulesYaml1 = readYamlFile<{ hoistPattern: string }>(path.resolve('project-1', 'node_modules', '.modules.yaml'))
  expect(modulesYaml1?.hoistPattern).toStrictEqual(['*'])

  const modulesYaml2 = readYamlFile<{ hoistPattern: string }>(path.resolve('project-2', 'node_modules', '.modules.yaml'))
  expect(modulesYaml2?.hoistPattern).toBeFalsy()
})

test('workspace .npmrc is always read', async () => {
  const projects = preparePackages([
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
  fs.writeFileSync('pnpm-workspace.yaml', '', 'utf8')
  fs.writeFileSync('.npmrc', 'shamefully-flatten = true\nshared-workspace-lockfile=false', 'utf8')
  fs.writeFileSync('workspace/project-2/.npmrc', 'hoist=false', 'utf8')

  process.chdir('workspace/project-1')
  await execPnpm(['install', '--store-dir', storeDir, '--filter', '.'])

  expect(projects['project-1'].requireModule('is-positive')).toBeTruthy()

  const modulesYaml1 = readYamlFile<{ hoistPattern: string }>(path.resolve('node_modules', '.modules.yaml'))
  expect(modulesYaml1?.hoistPattern).toStrictEqual(['*'])

  process.chdir('..')
  process.chdir('project-2')

  await execPnpm(['install', '--store-dir', storeDir, '--filter', '.'])

  expect(projects['project-2'].requireModule('is-negative')).toBeTruthy()

  const modulesYaml2 = readYamlFile<{ hoistPattern: string }>(path.resolve('node_modules', '.modules.yaml'))
  expect(modulesYaml2?.hoistPattern).toBeFalsy()
})

skipOnWindows('recursive installation using server', async () => {
  const projects = preparePackages([
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
  spawnPnpm(['server', 'start'], { storeDir })

  const serverJsonPath = path.resolve(storeDir, 'v3/server/server.json')
  const serverJson = await retryLoadJsonFile<{ connectionOptions: object }>(serverJsonPath)
  expect(serverJson).toBeTruthy()
  expect(serverJson.connectionOptions).toBeTruthy()

  await execPnpm(['recursive', 'install'])

  expect(projects['project-1'].requireModule('is-positive')).toBeTruthy()
  expect(projects['project-2'].requireModule('is-negative')).toBeTruthy()

  await execPnpm(['server', 'stop', '--store-dir', storeDir])
})

test('recursive installation of packages with hooks', async () => {
  // This test hangs on Appveyor for some reason
  if (isCI && isWindows()) return
  const projects = preparePackages([
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
      pkg.dependencies['@pnpm.e2e/dep-of-pkg-with-1-dep'] = '100.1.0'
      return pkg
    }
  `
  fs.writeFileSync('.pnpmfile.cjs', pnpmfile, 'utf8')

  process.chdir('../project-2')
  fs.writeFileSync('.pnpmfile.cjs', pnpmfile, 'utf8')

  process.chdir('..')

  await execPnpm(['recursive', 'install'])

  const lockfile1 = projects['project-1'].readLockfile()
  expect(lockfile1.packages).toHaveProperty(['/@pnpm.e2e/dep-of-pkg-with-1-dep@100.1.0'])

  const lockfile2 = projects['project-2'].readLockfile()
  expect(lockfile2.packages).toHaveProperty(['/@pnpm.e2e/dep-of-pkg-with-1-dep@100.1.0'])
})

test('recursive installation of packages in workspace ignores hooks in packages', async () => {
  // This test hangs on Appveyor for some reason
  if (isCI && isWindows()) return
  preparePackages([
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

  const pnpmfile = `
    module.exports = { hooks: { readPackage } }
    function readPackage (pkg) {
      pkg.dependencies = pkg.dependencies || {}
      pkg.dependencies['@pnpm.e2e/dep-of-pkg-with-1-dep'] = '100.1.0'
      return pkg
    }
  `
  fs.writeFileSync('project-1/.pnpmfile.cjs', pnpmfile, 'utf8')
  fs.writeFileSync('project-2/.pnpmfile.cjs', pnpmfile, 'utf8')
  fs.writeFileSync('.pnpmfile.cjs', `
    module.exports = { hooks: { readPackage } }
    function readPackage (pkg) {
      pkg.dependencies = pkg.dependencies || {}
      pkg.dependencies['is-number'] = '1.0.0'
      return pkg
    }
  `)

  writeYamlFile('pnpm-workspace.yaml', { packages: ['project-1', 'project-2'] })

  await execPnpm(['install'])

  const lockfile = readYamlFile<Lockfile>('pnpm-lock.yaml')
  const depPaths = Object.keys(lockfile.packages ?? [])
  expect(depPaths).not.toContain('/@pnpm.e2e/dep-of-pkg-with-1-dep@100.1.0')
  expect(depPaths).toContain('/is-number@1.0.0')
})

test('ignores .pnpmfile.cjs during recursive installation when --ignore-pnpmfile is used', async () => {
  // This test hangs on Appveyor for some reason
  if (isCI && isWindows()) return
  const projects = preparePackages([
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
      pkg.dependencies['@pnpm.e2e/dep-of-pkg-with-1-dep'] = '100.1.0'
      return pkg
    }
  `
  fs.writeFileSync('.pnpmfile.cjs', pnpmfile, 'utf8')

  process.chdir('../project-2')
  fs.writeFileSync('.pnpmfile.cjs', pnpmfile, 'utf8')

  process.chdir('..')

  await execPnpm(['recursive', 'install', '--ignore-pnpmfile'])

  const lockfile1 = projects['project-1'].readLockfile()
  expect(lockfile1.packages).not.toHaveProperty(['/@pnpm.e2e/dep-of-pkg-with-1-dep@100.1.0'])

  const lockfile2 = projects['project-2'].readLockfile()
  expect(lockfile2.packages).not.toHaveProperty(['/@pnpm.e2e/dep-of-pkg-with-1-dep@100.1.0'])
})

test('recursive command with filter from config', async () => {
  const projects = preparePackages([
    {
      name: 'project-1',
      version: '1.0.0',

      dependencies: {
        'is-positive': '1.0.0',
        'project-2': 'workspace:*',
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

  fs.writeFileSync('package.json', '{}', 'utf8')
  fs.writeFileSync('.npmrc', 'filter=project-1 project-2', 'utf8')
  await execPnpm(['recursive', 'install'])

  projects['project-1'].has('is-positive')
  projects['project-2'].has('is-negative')
  projects['project-3'].hasNot('minimatch')
})

test('non-recursive install ignores filter from config', async () => {
  const projects = preparePackages([
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

  fs.writeFileSync('.npmrc', 'filter=project-2', 'utf8')
  await execPnpm(['install'])

  projects['project-1'].has('is-positive')
  projects['project-2'].hasNot('is-negative')
  projects['project-3'].hasNot('minimatch')
})

test('adding new dependency in the root should fail if neither --workspace-root nor --ignore-workspace-root-check are used', async () => {
  const project = prepare()

  fs.writeFileSync('pnpm-workspace.yaml', '', 'utf8')

  {
    const { status, stdout } = execPnpmSync(['add', 'is-positive'])

    expect(status).toBe(1)

    expect(stdout.toString()).toMatch(/Running this command will add the dependency to the workspace root, which might not be what you want - if you really meant it, /)
  }

  {
    const { status } = execPnpmSync(['add', 'is-positive', '--ignore-workspace-root-check'])

    expect(status).toBe(0)
    project.has('is-positive')
  }

  {
    const { status } = execPnpmSync(['add', 'is-odd', '--workspace-root'])

    expect(status).toBe(0)
    project.has('is-odd')
  }

  {
    const { status } = execPnpmSync(['add', 'is-even', '-w'])

    expect(status).toBe(0)
    project.has('is-even')
  }
})

test('--workspace-packages', async () => {
  const projects = preparePackages([
    {
      location: 'project-1',
      package: {
        name: 'project-1',
        version: '1.0.0',

        dependencies: {
          'is-positive': '1.0.0',
        },
      },
    },
    {
      location: 'project-2',
      package: {
        name: 'project-2',
        version: '1.0.0',

        dependencies: {
          'is-positive': '1.0.0',
        },
      },
    },
  ])

  const storeDir = path.resolve('../store')
  fs.writeFileSync('pnpm-workspace.yaml', '', 'utf8')

  await execPnpm(['install', '--store-dir', storeDir, '--workspace-packages', 'project-1'])

  projects['project-1'].has('is-positive')
  projects['project-2'].hasNot('is-positive')
})

test('set recursive-install to false in .npmrc would disable recursive install in workspace', async () => {
  const projects = preparePackages([
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

  process.chdir('workspace')
  fs.writeFileSync('pnpm-workspace.yaml', '', 'utf8')
  fs.writeFileSync('.npmrc', `recursive-install = false
dedupe-peer-dependents = false`, 'utf8')

  process.chdir('project-1')
  await execPnpm(['install'])

  projects['project-1'].has('is-positive')
  projects['project-2'].hasNot('is-negative')
})

test('set recursive-install to false would install as --filter {.}...', async () => {
  const projects = preparePackages([
    {
      location: 'workspace/project-1',
      package: {
        name: 'project-1',
        version: '1.0.0',

        dependencies: {
          'project-2': 'workspace:*',
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

  process.chdir('workspace')
  fs.writeFileSync('pnpm-workspace.yaml', '', 'utf8')
  fs.writeFileSync('.npmrc', 'recursive-install = false', 'utf8')

  process.chdir('project-1')
  await execPnpm(['install'])

  projects['project-2'].has('is-negative')
})
