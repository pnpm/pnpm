import fs from 'node:fs'
import http from 'node:http'
import type { AddressInfo, Socket } from 'node:net'
import path from 'node:path'

import { afterAll, expect, test } from '@jest/globals'
import { STORE_VERSION, WANTED_LOCKFILE } from '@pnpm/constants'
import type { LockfileObject } from '@pnpm/lockfile.types'
import { readPackageJsonFromDir } from '@pnpm/pkg-manifest.reader'
import { prepare, prepareEmpty, preparePackages } from '@pnpm/prepare'
import type { PackageFilesIndex } from '@pnpm/store.cafs'
import { StoreIndex, storeIndexKey } from '@pnpm/store.index'
import { fixtures } from '@pnpm/test-fixtures'
import { getIntegrity } from '@pnpm/testing.registry-mock'
import { lexCompare } from '@pnpm/text.ordinal-comparator'
import { readProjectManifest } from '@pnpm/workspace.project-manifest-reader'
import { writeProjectManifest } from '@pnpm/workspace.project-manifest-writer'
import { rimrafSync } from '@zkochan/rimraf'
import crossSpawn from 'cross-spawn'
import { dirIsCaseSensitive } from 'dir-is-case-sensitive'
import isWindows from 'is-windows'
import { readYamlFileSync } from 'read-yaml-file'
import { writeYamlFileSync } from 'write-yaml-file'

import {
  execPnpm,
  execPnpmSync,
} from '../utils/index.js'

const skipOnWindows = isWindows() ? test.skip : test
const f = fixtures(import.meta.dirname)

const storeIndexes: StoreIndex[] = []
afterAll(() => {
  for (const si of storeIndexes) si.close()
})

// Covers https://github.com/pnpm/pnpm/issues/895 and https://github.com/pnpm/pnpm/issues/9512
test('install relinks dependencies after the project directory is moved', async () => {
  prepare({
    dependencies: {
      'is-positive': '1.0.0',
    },
  })
  await execPnpm(['install'])

  // Junctions, which pnpm uses on Windows without the symlink privilege,
  // point at absolute paths that break when the project is moved.
  fs.unlinkSync('node_modules/is-positive')
  fs.symlinkSync(path.resolve('node_modules/.pnpm/is-positive@1.0.0/node_modules/is-positive'), 'node_modules/is-positive', 'junction')
  const projectDir = process.cwd()
  const movedDir = path.resolve('../moved-project')
  process.chdir('..')
  fs.renameSync(projectDir, movedDir)
  process.chdir(movedDir)
  expect(fs.existsSync('node_modules/is-positive/package.json')).toBe(false)

  await execPnpm(['install', '--config.confirm-modules-purge=false'])

  expect(fs.existsSync('node_modules/is-positive/package.json')).toBe(true)
})

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

  writeYamlFileSync('pnpm-workspace.yaml', {
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

  await execPnpm(['install', 'is-positive', '--lockfile-dir', path.resolve('..')])

  project.has('is-positive')

  const lockfile = readYamlFileSync<LockfileObject>(path.resolve('..', WANTED_LOCKFILE))

  expect(Object.keys(lockfile.importers)).toStrictEqual(['project'])
})

test('install --save-exact', async () => {
  const project = prepare()

  await execPnpm(['install', 'is-positive@3.1.0', '--save-exact', '--save-dev'])

  project.has('is-positive')

  const pkg = await readPackageJsonFromDir(process.cwd())

  expect(pkg.devDependencies).toStrictEqual({ 'is-positive': '3.1.0' })
})

test('install keeps an empty peerDependencies field in package.json', async () => {
  prepareEmpty()
  fs.writeFileSync('package.json', JSON.stringify({ name: 'project', version: '0.0.0', peerDependencies: {} }), 'utf8')

  await execPnpm(['install', 'is-positive@3.1.0', '--save-exact'])

  const pkg = JSON.parse(fs.readFileSync('package.json', 'utf8'))

  expect(pkg.peerDependencies).toStrictEqual({})
  expect(pkg.dependencies).toStrictEqual({ 'is-positive': '3.1.0' })
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

  const storeDir = project.getStorePath()
  const indexKey = storeIndexKey(getIntegrity('@pnpm.e2e/with-same-file-in-different-cases', '1.0.0'), '@pnpm.e2e/with-same-file-in-different-cases@1.0.0')
  const si = new StoreIndex(storeDir)
  let filesIndex: PackageFilesIndex
  try {
    filesIndex = si.get(indexKey) as PackageFilesIndex
  } finally {
    si.close()
  }
  const packageFiles = Array.from(filesIndex.files.keys()).sort(lexCompare)

  expect(packageFiles).toStrictEqual(['Foo.js', 'LICENSE', 'foo.js', 'package.json'])
  const files = fs.readdirSync('node_modules/@pnpm.e2e/with-same-file-in-different-cases')
  if (await dirIsCaseSensitive(storeDir)) {
    expect([...files].sort(lexCompare)).toStrictEqual(['Foo.js', 'LICENSE', 'foo.js', 'package.json'])
  } else {
    expect([...files].map((f) => f.toLowerCase()).sort(lexCompare)).toStrictEqual(['foo.js', 'license', 'package.json'])
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

  rimrafSync('node_modules')
  rimrafSync('.pnpm')

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

  rimrafSync('node_modules')
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
  const registry = await startStalledRegistry()

  try {
    await expect(
      execPnpm(['add', 'typescript@2.4.2', `--registry=${registry.url}`, '--fetch-timeout=500', '--fetch-retries=0'])
    ).rejects.toThrow('ERR_PNPM_META_FETCH_FAIL')
    expect(registry.requestCount()).toBeGreaterThan(0)
  } finally {
    registry.close()
  }
})

interface StalledRegistry {
  url: string
  requestCount: () => number
  close: () => void
}

/**
 * A registry that accepts the request and never answers it, so the fetch
 * timeout is the only thing that can end the install.
 */
async function startStalledRegistry (): Promise<StalledRegistry> {
  const sockets = new Set<Socket>()
  let requests = 0
  const server = http.createServer(() => {
    requests++
  })
  server.on('connection', (socket) => {
    sockets.add(socket)
    socket.on('close', () => {
      sockets.delete(socket)
    })
  })
  await new Promise<void>((resolve) => {
    server.listen(0, '127.0.0.1', resolve)
  })
  return {
    url: `http://127.0.0.1:${(server.address() as AddressInfo).port}/`,
    requestCount: () => requests,
    close: () => {
      for (const socket of sockets) socket.destroy()
      server.close()
    },
  }
}

test('installation fails when the stored package name and version do not match the meta of the installed package', async () => {
  prepare()
  const storeDir = path.resolve('store')
  const settings = [`--config.store-dir=${storeDir}`]

  await execPnpm(['add', '@pnpm.e2e/dep-of-pkg-with-1-dep@100.1.0', ...settings])

  const cacheIntegrityKey = storeIndexKey(getIntegrity('@pnpm.e2e/dep-of-pkg-with-1-dep', '100.1.0'), '@pnpm.e2e/dep-of-pkg-with-1-dep@100.1.0')
  const storeIndex = new StoreIndex(path.join(storeDir, STORE_VERSION))
  storeIndexes.push(storeIndex)
  const cacheIntegrity = storeIndex.get(cacheIntegrityKey) as PackageFilesIndex
  storeIndex.set(cacheIntegrityKey, {
    ...cacheIntegrity,
    manifest: { ...cacheIntegrity.manifest, name: 'foo' },
  })

  rimrafSync('node_modules')
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

test('lockfile verifier rejects a trust-downgraded entry that bypassed resolution', () => {
  // Step 1: install with trust policy off. The resolver picks up the
  // downgraded version without complaint and writes it to the lockfile.
  prepare()
  execPnpmSync(
    ['add', '@pnpm/e2e.test-provenance@0.0.5', '--trust-policy=off'],
    { expectSuccess: true }
  )

  // Step 2: turn the policy on. The resolver wouldn't be invoked under
  // --frozen-lockfile (peek-path takes over), so the trust check would
  // be silently bypassed if the verifier weren't running. The lockfile
  // verifier catches the same downgrade pattern the resolver-time check
  // catches at fresh resolution.
  const result = execPnpmSync([
    'install',
    '--frozen-lockfile',
    '--trust-policy=no-downgrade',
  ])
  expect(result.status).toBe(1)
  const output = `${result.stdout.toString()}\n${result.stderr.toString()}`
  expect(output).toContain('ERR_PNPM_TRUST_DOWNGRADE')
  expect(output).toMatch(/@pnpm\/e2e\.test-provenance/)
})

test('lockfile verifier respects trust-policy-exclude on a downgraded lockfile entry', () => {
  prepare()
  execPnpmSync(
    ['add', '@pnpm/e2e.test-provenance@0.0.5', '--trust-policy=off'],
    { expectSuccess: true }
  )

  // With the exclude entry in place, the verifier short-circuits before
  // running the trust check on this package — mirrors the resolver-time
  // exclude path so users have one consistent way to allow a downgrade.
  execPnpmSync([
    'install',
    '--frozen-lockfile',
    '--trust-policy=no-downgrade',
    '--trust-policy-exclude=@pnpm/e2e.test-provenance',
  ], { expectSuccess: true })
})

test('trustPolicyExclude set to a single string in pnpm-workspace.yaml excludes that package', () => {
  prepare()
  execPnpmSync(
    ['add', '@pnpm/e2e.test-provenance@0.0.5', '--trust-policy=off'],
    { expectSuccess: true }
  )

  writeYamlFileSync('pnpm-workspace.yaml', {
    trustPolicy: 'no-downgrade',
    trustPolicyExclude: '@pnpm/e2e.test-provenance@0.0.5',
  })

  execPnpmSync([
    'install',
    '--lockfile-only',
  ], { expectSuccess: true })
})

// Windows CI volumes do not support explicit clone imports.
const forceRepairImportMethods = isWindows()
  ? ['auto', 'hardlink', 'copy']
  : ['auto', 'hardlink', 'copy', 'clone']

// Covers https://github.com/pnpm/pnpm/issues/919
test.each(forceRepairImportMethods)('install --force restores a replaced dependency file in node_modules (packageImportMethod=%s)', async (packageImportMethod) => {
  prepare({
    dependencies: {
      'is-positive': '1.0.0',
    },
  })
  const env = { pnpm_config_package_import_method: packageImportMethod }

  await execPnpm(['install'], { env })

  const installedFile = path.resolve('node_modules/is-positive/index.js')
  const pristine = fs.readFileSync(installedFile, 'utf8')
  // Replace the file rather than writing through it, so the store stays intact
  // under every import method.
  fs.rmSync(installedFile)
  fs.writeFileSync(installedFile, `${pristine}\n// tampered\n`, 'utf8')

  await execPnpm(['install', '--force'], { env })

  expect(fs.readFileSync(installedFile, 'utf8')).toBe(pristine)
})

// Covers https://github.com/pnpm/pnpm/issues/919
test('install --force refetches a dependency whose store content was modified too', async () => {
  prepare({
    dependencies: {
      'is-positive': '1.0.0',
      'is-negative': '1.0.0',
    },
  })
  const env = { pnpm_config_package_import_method: 'hardlink' }

  await execPnpm(['install'], { env })

  const installedFile = path.resolve('node_modules/is-positive/index.js')
  const pristine = fs.readFileSync(installedFile, 'utf8')
  // Append through the hardlink, which mutates the store's copy as well.
  fs.appendFileSync(installedFile, '\n// tampered\n', 'utf8')
  // The store skips verifying a file whose mtime is within 100ms of the last
  // check, so move it past that window to make the edit observable.
  const afterTheSkipWindow = new Date(Date.now() + 60_000)
  fs.utimesSync(installedFile, afterTheSkipWindow, afterTheSkipWindow)

  await execPnpm(['install', '--force'], { env })

  expect(fs.readFileSync(installedFile, 'utf8')).toBe(pristine)
  expect(execPnpmSync(['store', 'status']).status).toBe(0)
})

// Covers https://github.com/pnpm/pnpm/issues/919
test('install --force reports the frozenStore conflict on a repeat install', async () => {
  prepare({
    dependencies: {
      'is-positive': '1.0.0',
    },
  })

  await execPnpm(['install'])

  const { status, stdout } = execPnpmSync(['install', '--force', '--frozen-store'])

  expect(status).toBe(1)
  expect(stdout.toString()).toContain('Cannot use force together with frozenStore')
})

test('adding a dependency succeeds after deleting offline package source', async () => {
  const project = prepareEmpty()

  const pkgDir = path.resolve('..', 'offline-pkg')
  fs.mkdirSync(path.join(pkgDir, 'package'), { recursive: true })
  fs.writeFileSync(path.join(pkgDir, 'package', 'package.json'), JSON.stringify({
    name: 'offline-pkg',
    version: '1.0.0',
    dependencies: {
      'is-positive': '1.0.0',
    },
  }))
  execPnpmSync(['pack', '--pack-destination', pkgDir], { cwd: path.join(pkgDir, 'package') })
  const tarball = path.join(pkgDir, 'offline-pkg-1.0.0.tgz')

  await execPnpm(['add', tarball])
  project.has('offline-pkg')
  let lockfile = project.readLockfile()
  expect(lockfile.packages['is-positive@1.0.0']).toBeDefined()

  fs.unlinkSync(tarball)

  await execPnpm(['add', '@pnpm.e2e/dep-of-pkg-with-1-dep@100.1.0'])

  project.has('offline-pkg')
  project.has('@pnpm.e2e/dep-of-pkg-with-1-dep')
  lockfile = project.readLockfile()
  expect(lockfile.packages['is-positive@1.0.0']).toBeDefined()
})

