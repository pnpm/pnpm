import { promises as fs } from 'fs'
import path from 'path'
import { WANTED_LOCKFILE } from '@pnpm/constants'
import { Lockfile } from '@pnpm/lockfile-types'
import prepare, { prepareEmpty, preparePackages } from '@pnpm/prepare'
import { fromDir as readPackageJsonFromDir } from '@pnpm/read-package-json'
import readProjectManifest from '@pnpm/read-project-manifest'
import writeProjectManifest from '@pnpm/write-project-manifest'
import dirIsCaseSensitive from 'dir-is-case-sensitive'
import readYamlFile from 'read-yaml-file'
import rimraf from '@zkochan/rimraf'
import isWindows from 'is-windows'
import loadJsonFile from 'load-json-file'
import exists from 'path-exists'
import crossSpawn from 'cross-spawn'
import {
  execPnpm,
  execPnpmSync,
} from '../utils'

const skipOnWindows = isWindows() ? test.skip : test

test('bin files are found by lifecycle scripts', () => {
  prepare({
    dependencies: {
      'hello-world-js-bin': '*',
    },
    scripts: {
      postinstall: 'hello-world-js-bin',
    },
  })

  const result = execPnpmSync(['install'])

  expect(result.status).toBe(0)
  expect(result.stdout.toString()).toContain('Hello world!')
})

skipOnWindows('create a "node_modules/.pnpm-debug.log" file when the command fails', async () => {
  prepare()

  const result = execPnpmSync(['add', '@zkochan/i-do-not-exist'])

  expect(result.status).toBe(1)

  expect(await exists('.pnpm-debug.log')).toBeTruthy()
})

skipOnWindows('install --lockfile-only', async () => {
  const project = prepare()

  await execPnpm(['install', 'rimraf@2.5.1', '--lockfile-only'])

  await project.hasNot('rimraf')

  const lockfile = await project.readLockfile()
  expect(lockfile.packages).toHaveProperty(['/rimraf/2.5.1'])
})

test('install --no-lockfile', async () => {
  const project = prepare()

  await execPnpm(['install', 'is-positive', '--no-lockfile'])

  await project.has('is-positive')

  expect(await project.readLockfile()).toBeFalsy()
})

test('install with package-lock=false in .npmrc', async () => {
  const project = prepare()

  await fs.writeFile('.npmrc', 'package-lock=false', 'utf8')

  await execPnpm(['add', 'is-positive'])

  await project.has('is-positive')

  expect(await project.readLockfile()).toBeFalsy()
})

test('install from any location via the --prefix flag', async () => {
  const project = prepare({
    dependencies: {
      rimraf: '2.6.2',
    },
  })

  process.chdir('..')

  await execPnpm(['install', '--prefix', 'project'])

  await project.has('rimraf')
  await project.isExecutable('.bin/rimraf')
})

test('install with external lockfile directory', async () => {
  const project = prepare()

  await execPnpm(['install', 'is-positive', '--lockfile-directory', path.resolve('..')])

  await project.has('is-positive')

  const lockfile = await readYamlFile<Lockfile>(path.resolve('..', WANTED_LOCKFILE))

  expect(Object.keys(lockfile.importers)).toStrictEqual(['project'])
})

test('install --save-exact', async () => {
  const project = prepare()

  await execPnpm(['install', 'is-positive@3.1.0', '--save-exact', '--save-dev'])

  await project.has('is-positive')

  const pkg = await readPackageJsonFromDir(process.cwd())

  expect(pkg.devDependencies).toStrictEqual({ 'is-positive': '3.1.0' })
})

test('install to a project that uses package.yaml', async () => {
  const project = prepareEmpty()

  await writeProjectManifest(path.resolve('package.yaml'), { name: 'foo', version: '1.0.0' })

  await execPnpm(['install', 'is-positive@3.1.0', '--save-exact', '--save-dev'])

  await project.has('is-positive')

  const { manifest } = await readProjectManifest(process.cwd())

  expect(manifest?.devDependencies).toStrictEqual({ 'is-positive': '3.1.0' })
})

test('install save new dep with the specified spec', async () => {
  const project = prepare()

  await execPnpm(['install', 'is-positive@~3.1.0'])

  await project.has('is-positive')

  const pkg = await readPackageJsonFromDir(process.cwd())

  expect(pkg.dependencies).toStrictEqual({ 'is-positive': '~3.1.0' })
})

// Covers https://github.com/pnpm/pnpm/issues/1685
test("don't fail on case insensitive filesystems when package has 2 files with same name", async () => {
  const project = prepare()

  await execPnpm(['install', 'with-same-file-in-different-cases'])

  await project.has('with-same-file-in-different-cases')

  const { files: integrityFile } = await loadJsonFile<{ files: object }>(await project.getPkgIndexFilePath('with-same-file-in-different-cases', '1.0.0'))
  const packageFiles = Object.keys(integrityFile).sort()

  expect(packageFiles).toStrictEqual(['Foo.js', 'foo.js', 'package.json'])
  const files = await fs.readdir('node_modules/with-same-file-in-different-cases')
  const storeDir = await project.getStorePath()
  if (await dirIsCaseSensitive(storeDir)) {
    expect([...files]).toStrictEqual(['Foo.js', 'foo.js', 'package.json'])
  } else {
    expect([...files]).toStrictEqual(['Foo.js', 'package.json'])
  }
})

test('top-level packages should find the plugins they use', async () => {
  prepare({
    scripts: {
      test: 'pkg-that-uses-plugins',
    },
  })

  await execPnpm(['install', 'pkg-that-uses-plugins', 'plugin-example'])

  const result = crossSpawn.sync('npm', ['test'])
  expect(result.stdout.toString()).toContain('My plugin is plugin-example')
  expect(result.status).toBe(0)
})

test('not top-level packages should find the plugins they use', async () => {
  // standard depends on eslint and eslint plugins
  prepare({
    scripts: {
      test: 'standard',
    },
  })

  await execPnpm(['install', 'standard@8.6.0'])

  const result = crossSpawn.sync('npm', ['test'])
  expect(result.status).toBe(0)
})

test('run js bin file', async () => {
  prepare({
    scripts: {
      test: 'hello-world-js-bin',
    },
  })

  await execPnpm(['install', 'hello-world-js-bin'])

  const result = crossSpawn.sync('npm', ['test'])
  expect(result.stdout.toString()).toContain('Hello world!')
  expect(result.status).toBe(0)
})

test('create a package.json if there is none', async () => {
  prepareEmpty()

  await execPnpm(['install', 'dep-of-pkg-with-1-dep@100.1.0'])

  expect((await import(path.resolve('package.json'))).default).toStrictEqual({
    dependencies: {
      'dep-of-pkg-with-1-dep': '100.1.0',
    },
  })
})

test('`pnpm add` should fail if no package name was provided', () => {
  prepare()

  const { status, stdout } = execPnpmSync(['add'])

  expect(status).toBe(1)
  expect(stdout.toString()).toContain('`pnpm add` requires the package name')
})

test('`pnpm recursive add` should fail if no package name was provided', () => {
  prepare()

  const { status, stdout } = execPnpmSync(['recursive', 'add'])

  expect(status).toBe(1)
  expect(stdout.toString()).toContain('`pnpm add` requires the package name')
})

test('install should fail if the used pnpm version does not satisfy the pnpm version specified in engines', async () => {
  prepare({
    name: 'project',
    version: '1.0.0',

    engines: {
      pnpm: '99999',
    },
  })

  const { status, stdout } = execPnpmSync(['install'])

  expect(status).toBe(1)
  expect(stdout.toString()).toContain('Your pnpm version is incompatible with')
})

test('engine-strict=false: install should not fail if the used Node version does not satisfy the Node version specified in engines', async () => {
  prepare({
    name: 'project',
    version: '1.0.0',

    engines: {
      node: '99999',
    },
  })

  const { status, stdout } = execPnpmSync(['install'])

  expect(status).toBe(0)
  expect(stdout.toString()).toContain('Unsupported engine')
})

test('engine-strict=true: install should fail if the used Node version does not satisfy the Node version specified in engines', async () => {
  prepare({
    name: 'project',
    version: '1.0.0',

    engines: {
      node: '99999',
    },
  })

  const { status, stdout } = execPnpmSync(['install', '--engine-strict'])

  expect(status).toBe(1)
  expect(stdout.toString()).toContain('Your Node version is incompatible with')
})

test('recursive install should fail if the used pnpm version does not satisfy the pnpm version specified in engines of any of the workspace projects', async () => {
  preparePackages([
    {
      name: 'project-1',
      version: '1.0.0',

      dependencies: {
        'is-positive': '1.0.0',
      },
      engines: {
        pnpm: '99999',
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

  const { status, stdout } = execPnpmSync(['recursive', 'install'])

  expect(status).toBe(1)
  expect(stdout.toString()).toContain('Your pnpm version is incompatible with')
})

test('engine-strict=true: recursive install should fail if the used Node version does not satisfy the Node version specified in engines of any of the workspace projects', async () => {
  preparePackages([
    {
      name: 'project-1',
      version: '1.0.0',

      dependencies: {
        'is-positive': '1.0.0',
      },
      engines: {
        node: '99999',
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

  const { status, stdout } = execPnpmSync(['recursive', 'install', '--engine-strict'])

  expect(status).toBe(1)
  expect(stdout.toString()).toContain('Your Node version is incompatible with')
})

test('engine-strict=false: recursive install should not fail if the used Node version does not satisfy the Node version specified in engines of any of the workspace projects', async () => {
  preparePackages([
    {
      name: 'project-1',
      version: '1.0.0',

      dependencies: {
        'is-positive': '1.0.0',
      },
      engines: {
        node: '99999',
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

  const { status, stdout } = execPnpmSync(['recursive', 'install'])

  expect(status).toBe(0)
  expect(stdout.toString()).toContain('Unsupported engine')
})

test('using a custom virtual-store-dir location', async () => {
  prepare({
    dependencies: { rimraf: '2.5.1' },
  })

  await execPnpm(['install', '--virtual-store-dir=.pnpm'])

  expect(await exists('.pnpm/rimraf@2.5.1/node_modules/rimraf/package.json')).toBeTruthy()
  expect(await exists('.pnpm/lock.yaml')).toBeTruthy()
  expect(await exists('.pnpm/node_modules/once/package.json')).toBeTruthy()

  await rimraf('node_modules')
  await rimraf('.pnpm')

  await execPnpm(['install', '--virtual-store-dir=.pnpm', '--frozen-lockfile'])

  expect(await exists('.pnpm/rimraf@2.5.1/node_modules/rimraf/package.json')).toBeTruthy()
  expect(await exists('.pnpm/lock.yaml')).toBeTruthy()
  expect(await exists('.pnpm/node_modules/once/package.json')).toBeTruthy()
})

// This is an integration test only because it is hard to mock is-ci
test('installing in a CI environment', async () => {
  const project = prepare({
    dependencies: { rimraf: '2.5.1' },
  })

  await execPnpm(['install'], { env: { CI: 'true' } })

  await project.writePackageJson({
    dependencies: { rimraf: '1' },
  })

  let err!: Error
  try {
    await execPnpm(['install'], { env: { CI: 'true' } })
  } catch (_err) {
    err = _err
  }
  expect(err).toBeTruthy()

  await execPnpm(['install', '--no-frozen-lockfile'], { env: { CI: 'true' } })

  await rimraf('node_modules')
  await project.writePackageJson({
    dependencies: { rimraf: '2' },
  })

  await execPnpm(['install', '--no-prefer-frozen-lockfile'], { env: { CI: 'true' } })
})
