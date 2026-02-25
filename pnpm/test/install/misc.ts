import fs from 'fs'
import path from 'path'
import { STORE_VERSION, WANTED_LOCKFILE } from '@pnpm/constants'
import { readMsgpackFileSync, writeMsgpackFileSync } from '@pnpm/fs.msgpack-file'
import { type LockfileObject } from '@pnpm/lockfile.types'
import { prepare, prepareEmpty, preparePackages } from '@pnpm/prepare'
import { readPackageJsonFromDir } from '@pnpm/read-package-json'
import { readProjectManifest } from '@pnpm/read-project-manifest'
import { getIntegrity } from '@pnpm/registry-mock'
import { getIndexFilePathInCafs, type PackageFilesIndex } from '@pnpm/store.cafs'
import { lexCompare } from '@pnpm/util.lex-comparator'
import { writeProjectManifest } from '@pnpm/write-project-manifest'
import { fixtures } from '@pnpm/test-fixtures'
import dirIsCaseSensitive from 'dir-is-case-sensitive'
import { sync as readYamlFile } from 'read-yaml-file'
import { sync as rimraf } from '@zkochan/rimraf'
import isWindows from 'is-windows'
import { sync as writeYamlFile } from 'write-yaml-file'
import crossSpawn from 'cross-spawn'
import {
  execPnpm,
  execPnpmSync,
} from '../utils/index.js'

const skipOnWindows = isWindows() ? test.skip : test
const f = fixtures(import.meta.dirname)

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
  expect(lockfile.packages).toHaveProperty(['rimraf@2.5.1'])
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

test('install with lockfile being false in pnpm-workspace.yaml', async () => {
  const project = prepare()

  writeYamlFile('pnpm-workspace.yaml', {
    lockfile: false,
  })

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

  const lockfile = readYamlFile<LockfileObject>(path.resolve('..', WANTED_LOCKFILE))

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

  const { files: integrityFile } = readMsgpackFileSync<PackageFilesIndex>(project.getPkgIndexFilePath('@pnpm.e2e/with-same-file-in-different-cases', '1.0.0'))
  const packageFiles = Array.from(integrityFile.keys()).sort(lexCompare)

  expect(packageFiles).toStrictEqual(['Foo.js', 'foo.js', 'package.json'])
  const files = fs.readdirSync('node_modules/@pnpm.e2e/with-same-file-in-different-cases')
  const storeDir = project.getStorePath()
  if (await dirIsCaseSensitive.default(storeDir)) {
    expect([...files].sort(lexCompare)).toStrictEqual(['Foo.js', 'foo.js', 'package.json'])
  } else {
    expect([...files].map((f) => f.toLowerCase()).sort(lexCompare)).toStrictEqual(['foo.js', 'package.json'])
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
  fs.writeFileSync('pnpm-workspace.yaml', 'allowBuilds: { "es5-ext": false }', 'utf8')

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

  expect((await import(path.resolve('package.json'))).default).toEqual({
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

  fs.writeFileSync('pnpm-workspace.yaml', `packages:
  - project`, 'utf8')

  const { status, stdout } = execPnpmSync(['-r', 'add'])

  expect(status).toBe(1)
  expect(stdout.toString()).toContain('`pnpm add` requires the package name')
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

  fs.writeFileSync('pnpm-workspace.yaml', `packages:
  - "*"`, 'utf8')

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

  fs.writeFileSync('pnpm-workspace.yaml', `packages:
  - "*"`, 'utf8')

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

  fs.writeFileSync('pnpm-workspace.yaml', `packages:
  - "*"`, 'utf8')

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
  expect(fs.existsSync('node_modules/.pnpm/lock.yaml')).toBeTruthy()
  expect(fs.existsSync('.pnpm/node_modules/once/package.json')).toBeTruthy()

  rimraf('node_modules')
  rimraf('.pnpm')

  await execPnpm(['install', '--virtual-store-dir=.pnpm', '--frozen-lockfile'])

  expect(fs.existsSync('.pnpm/rimraf@2.5.1/node_modules/rimraf/package.json')).toBeTruthy()
  expect(fs.existsSync('node_modules/.pnpm/lock.yaml')).toBeTruthy()
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

// Tests for issue #9861: frozen-lockfile should be overridable via env vars and updateConfig hook
test('CI mode: frozen-lockfile can be overridden via environment variable', async () => {
  const project = prepare({
    dependencies: { rimraf: '2.5.1' },
  })

  // Initial install in CI mode
  await execPnpm(['install'], { env: { CI: 'true' } })

  // Change dependencies
  project.writePackageJson({
    dependencies: { rimraf: '1' },
  })

  // Should not fail when pnpm_config_frozen_lockfile is set to false
  await execPnpm(['install'], {
    env: {
      CI: 'true',
      pnpm_config_frozen_lockfile: 'false',
    },
  })
})

test('CI mode: frozen-lockfile can be overridden via updateConfig hook', async () => {
  const project = prepare({
    dependencies: { rimraf: '2.5.1' },
  })

  const pnpmfile = `
    module.exports = {
      hooks: {
        updateConfig(config) {
          config.frozenLockfile = false
          return config
        }
      }
    }
  `
  fs.writeFileSync('.pnpmfile.cjs', pnpmfile, 'utf8')

  // Initial install in CI mode
  await execPnpm(['install'], { env: { CI: 'true' } })

  // Change dependencies
  project.writePackageJson({
    dependencies: { rimraf: '1' },
  })

  // Should not fail due to updateConfig hook setting frozenLockfile to false
  await execPnpm(['install'], { env: { CI: 'true' } })
})

test('installation fails with a timeout error', async () => {
  prepare()

  await expect(
    execPnpm(['add', 'typescript@2.4.2', '--fetch-timeout=1', '--fetch-retries=0'])
  ).rejects.toThrow()
})

test('installation fails when the stored package name and version do not match the meta of the installed package', async () => {
  prepare()
  const storeDir = path.resolve('store')
  const settings = [`--config.store-dir=${storeDir}`]

  await execPnpm(['add', '@pnpm.e2e/dep-of-pkg-with-1-dep@100.1.0', ...settings])

  const cacheIntegrityPath = getIndexFilePathInCafs(path.join(storeDir, STORE_VERSION), getIntegrity('@pnpm.e2e/dep-of-pkg-with-1-dep', '100.1.0'), '@pnpm.e2e/dep-of-pkg-with-1-dep@100.1.0')
  const cacheIntegrity = readMsgpackFileSync<PackageFilesIndex>(cacheIntegrityPath)
  writeMsgpackFileSync(cacheIntegrityPath, {
    ...cacheIntegrity,
    manifest: { ...cacheIntegrity.manifest, name: 'foo' },
  })

  rimraf('node_modules')
  await expect(
    execPnpm(['install', ...settings])
  ).rejects.toThrow()

  await execPnpm(['install', '--config.strict-store-pkg-content-check=false', ...settings])
})

// Covers https://github.com/pnpm/pnpm/issues/8538
test('do not fail to render peer dependencies warning, when cache was hit during peer resolution', () => {
  prepare({
    dependencies: {
      '@udecode/plate-ui-table': '18.15.0',
      '@udecode/plate-ui-toolbar': '18.15.0',
    },
  })

  const result = execPnpmSync(['install', '--config.auto-install-peers=false'])

  expect(result.status).toBe(0)
  expect(result.stdout.toString()).toContain('Issues with peer dependencies found')
})

// Covers https://github.com/pnpm/pnpm/issues/8720
test('do not hang on circular peer dependencies', () => {
  const tempDir = f.prepare('workspace-with-circular-peers')
  process.chdir(tempDir)

  const result = execPnpmSync(['install', '--lockfile-only'])

  expect(result.status).toBe(0)
  expect(fs.existsSync(path.join(tempDir, WANTED_LOCKFILE))).toBeTruthy()
})

// Covers https://github.com/pnpm/pnpm/issues/7697
test('install success even though the url\'s hash contains slash', async () => {
  prepare()
  const settings = ['--fetch-retries=0']
  const result = execPnpmSync([
    'add',
    'https://github.com/pnpm-e2e/simple-pkg.git#branch/with-slash',
    ...settings,
  ])
  expect(result.status).toBe(0)
})

test('install fails when the trust evidence of a package is downgraded', async () => {
  const project = prepare()
  const result = execPnpmSync([
    'add',
    '@pnpm/e2e.test-provenance@0.0.5',
    '--trust-policy=no-downgrade',
  ])
  expect(result.status).toBe(1)
  project.hasNot('@pnpm/e2e.test-provenance')
})

test('install does not fail when the trust evidence of a package is downgraded but trust-policy is turned off', async () => {
  const project = prepare()
  const result = execPnpmSync([
    'add',
    '@pnpm/e2e.test-provenance@0.0.5',
    '--trust-policy=off',
  ])
  expect(result.status).toBe(0)
  project.has('@pnpm/e2e.test-provenance')
})

test('install does not fail when the trust evidence of a package is downgraded but it is in trust-policy-exclude', async () => {
  const project = prepare()
  const result = execPnpmSync([
    'add',
    '@pnpm/e2e.test-provenance@0.0.5',
    '--trust-policy=no-downgrade',
    '--trust-policy-exclude=@pnpm/e2e.test-provenance@0.0.5',
  ])
  expect(result.status).toBe(0)
  project.has('@pnpm/e2e.test-provenance')
})

test('install does not fail when the trust evidence of a package is downgraded but the package name is in trust-policy-exclude', async () => {
  const project = prepare()
  const result = execPnpmSync([
    'add',
    '@pnpm/e2e.test-provenance@0.0.5',
    '--trust-policy=no-downgrade',
    '--trust-policy-exclude=@pnpm/e2e.test-provenance',
  ])
  expect(result.status).toBe(0)
  project.has('@pnpm/e2e.test-provenance')
})

test('install fails when trust evidence of an optional dependency is downgraded', async () => {
  prepare()
  const result = execPnpmSync([
    'add',
    '@pnpm.e2e/has-untrusted-optional-dep@1.0.0',
    '--trust-policy=no-downgrade',
  ])
  expect(result.stdout.toString()).toContain('ERR_PNPM_TRUST_DOWNGRADE')
  expect(result.status).toBe(1)
})

test('install does not fail when the trust evidence of a package is downgraded but the trust-policy-ignore-after is set', async () => {
  const project = prepare()
  const result = execPnpmSync([
    'add',
    '@pnpm/e2e.test-provenance@0.0.5',
    '--trust-policy=no-downgrade',
    '--trust-policy-ignore-after=1440', // 1 day
  ])
  expect(result.status).toBe(0)
  project.has('@pnpm/e2e.test-provenance')
})
