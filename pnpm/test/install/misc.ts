import fs from 'fs'
import path from 'path'
import { WANTED_LOCKFILE } from '@pnpm/constants'
import { type Lockfile } from '@pnpm/lockfile-types'
import { prepare, prepareEmpty, preparePackages } from '@pnpm/prepare'
import { readPackageJsonFromDir } from '@pnpm/read-package-json'
import { readProjectManifest } from '@pnpm/read-project-manifest'
import { writeProjectManifest } from '@pnpm/write-project-manifest'
import dirIsCaseSensitive from 'dir-is-case-sensitive'
import { sync as readYamlFile } from 'read-yaml-file'
import { sync as rimraf } from '@zkochan/rimraf'
import isWindows from 'is-windows'
import loadJsonFile from 'load-json-file'
import crossSpawn from 'cross-spawn'
import {
  execPnpm,
  execPnpmSync,
} from '../utils'

const skipOnWindows = isWindows() ? test.skip : test

test('bin files are found by lifecycle scripts', () => {
  prepare({
    dependencies: {
      '@pnpm.e2e/hello-world-js-bin': '*',
    },
    scripts: {
      postinstall: 'hello-world-js-bin',
    },
  })

  const result = execPnpmSync(['install'])

  expect(result.status).toBe(0)
  expect(result.stdout.toString()).toContain('Hello world!')
})

skipOnWindows('install --lockfile-only', async () => {
  const project = prepare()

  await execPnpm(['install', 'rimraf@2.5.1', '--lockfile-only'])

  project.hasNot('rimraf')

  const lockfile = project.readLockfile()
  expect(lockfile.packages).toHaveProperty(['/rimraf@2.5.1'])
})

test('install --no-lockfile', async () => {
  const project = prepare()

  await execPnpm(['install', 'is-positive', '--no-lockfile'])

  project.has('is-positive')

  expect(project.readLockfile()).toBeFalsy()
})

test('write to stderr when --use-stderr is used', async () => {
  const project = prepare()

  const result = execPnpmSync(['add', 'is-positive', '--use-stderr'])

  project.has('is-positive')
  expect(result.stdout.toString()).toBe('')
  expect(result.stderr.toString()).not.toBe('')
})

test('install with package-lock=false in .npmrc', async () => {
  const project = prepare()

  fs.writeFileSync('.npmrc', 'package-lock=false', 'utf8')

  await execPnpm(['add', 'is-positive'])

  project.has('is-positive')

  expect(project.readLockfile()).toBeFalsy()
})

test('install from any location via the --prefix flag', async () => {
  const project = prepare({
    dependencies: {
      rimraf: '2.6.2',
    },
  })

  process.chdir('..')

  await execPnpm(['install', '--prefix', 'project'])

  project.has('rimraf')
  project.isExecutable('.bin/rimraf')
})

test('install with external lockfile directory', async () => {
  const project = prepare()

  await execPnpm(['install', 'is-positive', '--lockfile-directory', path.resolve('..')])

  project.has('is-positive')

  const lockfile = readYamlFile<Lockfile>(path.resolve('..', WANTED_LOCKFILE))

  expect(Object.keys(lockfile.importers)).toStrictEqual(['project'])
})

test('install --save-exact', async () => {
  const project = prepare()

  await execPnpm(['install', 'is-positive@3.1.0', '--save-exact', '--save-dev'])

  project.has('is-positive')

  const pkg = await readPackageJsonFromDir(process.cwd())

  expect(pkg.devDependencies).toStrictEqual({ 'is-positive': '3.1.0' })
})

test('install to a project that uses package.yaml', async () => {
  const project = prepareEmpty()

  await writeProjectManifest(path.resolve('package.yaml'), { name: 'foo', version: '1.0.0' })

  await execPnpm(['install', 'is-positive@3.1.0', '--save-exact', '--save-dev'])

  project.has('is-positive')

  const { manifest } = await readProjectManifest(process.cwd())

  expect(manifest?.devDependencies).toStrictEqual({ 'is-positive': '3.1.0' })
})

test('install save new dep with the specified spec', async () => {
  const project = prepare()

  await execPnpm(['install', 'is-positive@~3.1.0'])

  project.has('is-positive')

  const pkg = await readPackageJsonFromDir(process.cwd())

  expect(pkg.dependencies).toStrictEqual({ 'is-positive': '~3.1.0' })
})

// Covers https://github.com/pnpm/pnpm/issues/1685
test("don't fail on case insensitive filesystems when package has 2 files with same name", async () => {
  const project = prepare()

  await execPnpm(['install', '@pnpm.e2e/with-same-file-in-different-cases'])

  project.has('@pnpm.e2e/with-same-file-in-different-cases')

  const { files: integrityFile } = loadJsonFile.sync<{ files: object }>(project.getPkgIndexFilePath('@pnpm.e2e/with-same-file-in-different-cases', '1.0.0'))
  const packageFiles = Object.keys(integrityFile).sort()

  expect(packageFiles).toStrictEqual(['Foo.js', 'foo.js', 'package.json'])
  const files = fs.readdirSync('node_modules/@pnpm.e2e/with-same-file-in-different-cases')
  const storeDir = project.getStorePath()
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

  await execPnpm(['install', '@pnpm.e2e/pkg-that-uses-plugins', '@pnpm.e2e/plugin-example'])

  const result = crossSpawn.sync('npm', ['test'])
  expect(result.stdout.toString()).toContain('My plugin is @pnpm.e2e/plugin-example')
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

  await execPnpm(['install', '@pnpm.e2e/hello-world-js-bin'])

  const result = crossSpawn.sync('npm', ['test'])
  expect(result.stdout.toString()).toContain('Hello world!')
  expect(result.status).toBe(0)
})

test('create a package.json if there is none', async () => {
  prepareEmpty()

  await execPnpm(['install', '@pnpm.e2e/dep-of-pkg-with-1-dep@100.1.0'])

  expect((await import(path.resolve('package.json'))).default).toStrictEqual({
    dependencies: {
      '@pnpm.e2e/dep-of-pkg-with-1-dep': '100.1.0',
    },
  })
})

test('`pnpm add` should fail if no package name was provided', () => {
  prepare()

  const { status, stdout } = execPnpmSync(['add'])

  expect(status).toBe(1)
  expect(stdout.toString()).toContain('`pnpm add` requires the package name')
})

test('`pnpm -r add` should fail if no package name was provided', () => {
  preparePackages([
    {
      name: 'project',
      version: '1.0.0',
    },
  ])

  fs.writeFileSync('pnpm-workspace.yaml', '', 'utf8')

  const { status, stdout } = execPnpmSync(['-r', 'add'])

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

test('install should fail if the used pnpm version does not satisfy the pnpm version specified in packageManager', async () => {
  prepare({
    name: 'project',
    version: '1.0.0',

    packageManager: 'pnpm@0.0.0',
  })

  const { status, stdout } = execPnpmSync(['install'])

  expect(status).toBe(1)
  expect(stdout.toString()).toContain('This project is configured to use v0.0.0 of pnpm. Your current pnpm is')

  expect(execPnpmSync(['install', '--config.package-manager-strict=false']).status).toBe(0)
  expect(execPnpmSync(['install'], {
    env: {
      COREPACK_ENABLE_STRICT: '0',
    },
  }).status).toBe(0)
})

test('install should fail if the project requires a different package manager', async () => {
  prepare({
    name: 'project',
    version: '1.0.0',

    packageManager: 'yarn@4.0.0',
  })

  const { status, stdout } = execPnpmSync(['install'])

  expect(status).toBe(1)
  expect(stdout.toString()).toContain('This project is configured to use yarn')

  expect(execPnpmSync(['install', '--config.package-manager-strict=false']).status).toBe(0)
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

  fs.writeFileSync('pnpm-workspace.yaml', '', 'utf8')

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

  fs.writeFileSync('pnpm-workspace.yaml', '', 'utf8')

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

  fs.writeFileSync('pnpm-workspace.yaml', '', 'utf8')

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

  expect(fs.existsSync('.pnpm/rimraf@2.5.1/node_modules/rimraf/package.json')).toBeTruthy()
  expect(fs.existsSync('.pnpm/lock.yaml')).toBeTruthy()
  expect(fs.existsSync('.pnpm/node_modules/once/package.json')).toBeTruthy()

  rimraf('node_modules')
  rimraf('.pnpm')

  await execPnpm(['install', '--virtual-store-dir=.pnpm', '--frozen-lockfile'])

  expect(fs.existsSync('.pnpm/rimraf@2.5.1/node_modules/rimraf/package.json')).toBeTruthy()
  expect(fs.existsSync('.pnpm/lock.yaml')).toBeTruthy()
  expect(fs.existsSync('.pnpm/node_modules/once/package.json')).toBeTruthy()
})

// This is an integration test only because it is hard to mock is-ci
test('installing in a CI environment', async () => {
  const project = prepare({
    dependencies: { rimraf: '2.5.1' },
  })

  await execPnpm(['install'], { env: { CI: 'true' } })

  project.writePackageJson({
    dependencies: { rimraf: '1' },
  })

  let err!: Error
  try {
    await execPnpm(['install'], { env: { CI: 'true' } })
  } catch (_err: any) { // eslint-disable-line
    err = _err
  }
  expect(err).toBeTruthy()

  await execPnpm(['install', '--no-frozen-lockfile'], { env: { CI: 'true' } })

  rimraf('node_modules')
  project.writePackageJson({
    dependencies: { rimraf: '2' },
  })

  await execPnpm(['install', '--no-prefer-frozen-lockfile'], { env: { CI: 'true' } })
})

test('installation fails with a timeout error', async () => {
  prepare()

  await expect(
    execPnpm(['add', 'typescript@2.4.2', '--fetch-timeout=1', '--fetch-retries=0'])
  ).rejects.toThrow()
})
