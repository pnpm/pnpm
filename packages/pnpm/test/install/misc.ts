import { WANTED_LOCKFILE } from '@pnpm/constants'
import { Lockfile } from '@pnpm/lockfile-types'
import prepare, { prepareEmpty, preparePackages } from '@pnpm/prepare'
import readImporterManifest from '@pnpm/read-importer-manifest'
import { fromDir as readPackageJsonFromDir } from '@pnpm/read-package-json'
import writeImporterManifest from '@pnpm/write-importer-manifest'
import rimraf = require('@zkochan/rimraf')
import crossSpawn = require('cross-spawn')
import delay = require('delay')
import dirIsCaseSensitive from 'dir-is-case-sensitive'
import loadJsonFile = require('load-json-file')
import fs = require('mz/fs')
import path = require('path')
import exists = require('path-exists')
import readYamlFile from 'read-yaml-file'
import semver = require('semver')
import tape = require('tape')
import promisifyTape from 'tape-promise'
import {
  execPnpm,
  execPnpmSync,
} from '../utils'

const test = promisifyTape(tape)
const testOnly = promisifyTape(tape.only)

test('bin files are found by lifecycle scripts', t => {
  const project = prepare(t, {
    dependencies: {
      'hello-world-js-bin': '*'
    },
    scripts: {
      postinstall: 'hello-world-js-bin'
    },
  })

  const result = execPnpmSync('install')

  t.equal(result.status, 0, 'installation was successfull')
  t.ok(result.stdout.toString().includes('Hello world!'), 'postinstall script was executed')

  t.end()
})

test('create a pnpm-debug.log file when the command fails', async function (t) {
  const project = prepare(t)

  const result = execPnpmSync('install', '@zkochan/i-do-not-exist')

  t.equal(result.status, 1, 'install failed')

  t.ok(await exists('pnpm-debug.log'), 'log file created')

  t.end()
})

test('install --lockfile-only', async (t: tape.Test) => {
  const project = prepare(t)

  await execPnpm('install', 'rimraf@2.5.1', '--lockfile-only')

  await project.hasNot('rimraf')

  const lockfile = await project.readLockfile()
  t.ok(lockfile.packages['/rimraf/2.5.1'])
})

test('install --no-lockfile', async (t: tape.Test) => {
  const project = prepare(t)

  await execPnpm('install', 'is-positive', '--no-lockfile')

  await project.has('is-positive')

  t.notOk(await project.readLockfile(), `${WANTED_LOCKFILE} not created`)
})

test('install with package-lock=false in .npmrc', async (t: tape.Test) => {
  const project = prepare(t)

  await fs.writeFile('.npmrc', 'package-lock=false', 'utf8')

  await execPnpm('add', 'is-positive')

  await project.has('is-positive')

  t.notOk(await project.readLockfile(), `${WANTED_LOCKFILE} not created`)
})

test('install from any location via the --prefix flag', async (t: tape.Test) => {
  const project = prepare(t, {
    dependencies: {
      rimraf: '2.6.2',
    },
  })

  process.chdir('..')

  await execPnpm('install', '--prefix', 'project')

  await project.has('rimraf')
  await project.isExecutable('.bin/rimraf')
})

test('install with external lockfile directory', async (t: tape.Test) => {
  const project = prepare(t)

  await execPnpm('install', 'is-positive', '--lockfile-directory', path.resolve('..'))

  await project.has('is-positive')

  const lockfile = await readYamlFile<Lockfile>(path.resolve('..', WANTED_LOCKFILE))

  t.deepEqual(Object.keys(lockfile.importers), ['project'], 'lockfile created in correct location')
})

test('install --save-exact', async (t: tape.Test) => {
  const project = prepare(t)

  await execPnpm('install', 'is-positive@3.1.0', '--save-exact', '--save-dev')

  await project.has('is-positive')

  const pkg = await readPackageJsonFromDir(process.cwd())

  t.deepEqual(pkg.devDependencies, { 'is-positive': '3.1.0' })
})

test('install to a project that uses package.yaml', async (t: tape.Test) => {
  const project = prepareEmpty(t)

  await writeImporterManifest(path.resolve('package.yaml'), { name: 'foo', version: '1.0.0' })

  await execPnpm('install', 'is-positive@3.1.0', '--save-exact', '--save-dev')

  await project.has('is-positive')

  const { manifest } = await readImporterManifest(process.cwd())

  t.deepEqual(manifest && manifest.devDependencies, { 'is-positive': '3.1.0' })
})

test('install save new dep with the specified spec', async (t: tape.Test) => {
  const project = prepare(t)

  await execPnpm('install', 'is-positive@~3.1.0')

  await project.has('is-positive')

  const pkg = await readPackageJsonFromDir(process.cwd())

  t.deepEqual(pkg.dependencies, { 'is-positive': '~3.1.0' })
})

// Covers https://github.com/pnpm/pnpm/issues/1685
test("don't fail on case insensitive filesystems when package has 2 files with same name", async (t) => {
  const project = prepare(t)

  await execPnpm('install', 'with-same-file-in-different-cases')

  await project.has('with-same-file-in-different-cases')

  const storeDir = await project.getStorePath()
  const integrityFile = await import(path.join(storeDir, 'localhost+4873', 'with-same-file-in-different-cases', '1.0.0', 'integrity.json'))
  const packageFiles = Object.keys(integrityFile).sort()

  if (await dirIsCaseSensitive(storeDir)) {
    t.deepEqual(packageFiles, ['Foo.js', 'foo.js', 'package.json'])
  } else {
    t.deepEqual(packageFiles, ['foo.js', 'package.json'])
  }
})

test('lockfile compatibility', async (t: tape.Test) => {
  if (semver.satisfies(process.version, '4')) {
    t.skip("don't run on Node.js 4")
    return
  }
  prepare(t, { dependencies: { rimraf: '*' } })

  await execPnpm('install', 'rimraf@2.5.1')

  return new Promise((resolve, reject) => {
    const proc = crossSpawn.spawn('npm', ['shrinkwrap'])

    proc.on('error', reject)

    proc.on('close', (code: number) => {
      if (code > 0) return reject(new Error('Exit code ' + code))
      const wrap = JSON.parse(fs.readFileSync('npm-shrinkwrap.json', 'utf-8'))
      t.ok(wrap.dependencies.rimraf.version === '2.5.1',
        'npm shrinkwrap is successful')
      resolve()
    })
  })
})

test('support installing and uninstalling from the same store simultaneously', async (t: tape.Test) => {
  const project = prepare(t)

  await Promise.all([
    execPnpm('install', 'pkg-that-installs-slowly'),
    (async () => {
      await delay(500) // to be sure that lock was created

      await project.storeHasNot('pkg-that-installs-slowly')
      await execPnpm('uninstall', 'rimraf@2.5.1')

      await project.has('pkg-that-installs-slowly')
      await project.hasNot('rimraf')
    })(),
  ])
})

test('top-level packages should find the plugins they use', async (t: tape.Test) => {
  prepare(t, {
    scripts: {
      test: 'pkg-that-uses-plugins',
    },
  })

  await execPnpm('install', 'pkg-that-uses-plugins', 'plugin-example')

  const result = crossSpawn.sync('npm', ['test'])
  t.ok(result.stdout.toString().includes('My plugin is plugin-example'), 'package executable have found its plugin')
  t.equal(result.status, 0, 'executable exited with success')
})

test('not top-level packages should find the plugins they use', async (t: tape.Test) => {
  // standard depends on eslint and eslint plugins
  prepare(t, {
    scripts: {
      test: 'standard',
    },
  })

  await execPnpm('install', 'standard@8.6.0')

  const result = crossSpawn.sync('npm', ['test'])
  t.equal(result.status, 0, 'standard exited with success')
})

test('run js bin file', async (t: tape.Test) => {
  prepare(t, {
    scripts: {
      test: 'hello-world-js-bin',
    },
  })

  await execPnpm('install', 'hello-world-js-bin')

  const result = crossSpawn.sync('npm', ['test'])
  t.ok(result.stdout.toString().includes('Hello world!'), 'package executable printed its message')
  t.equal(result.status, 0, 'executable exited with success')
})

test('create a package.json if there is none', async (t: tape.Test) => {
  prepareEmpty(t)

  await execPnpm('install', 'dep-of-pkg-with-1-dep@100.1.0')

  t.deepEqual(await import(path.resolve('package.json')), {
    dependencies: {
      'dep-of-pkg-with-1-dep': '100.1.0',
    },
  }, 'package.json created')
})

test('pnpm install --save-peer', async (t) => {
  const project = prepare(t)

  await execPnpm('add', 'is-positive@1.0.0', '--save-peer')

  {
    const manifest = await loadJsonFile('package.json')

    t.deepEqual(
      manifest,
      {
        name: 'project',
        version: '0.0.0',

        devDependencies: { 'is-positive': '1.0.0' },
        peerDependencies: { 'is-positive': '1.0.0' },
      },
    )
  }

  await project.has('is-positive')

  await execPnpm('uninstall', 'is-positive')

  await project.hasNot('is-positive')

  {
    const manifest = await loadJsonFile('package.json')

    t.deepEqual(
      manifest,
      {
        name: 'project',
        version: '0.0.0',
      },
    )
  }
})

test('`pnpm add` should fail if no package name was provided', (t: tape.Test) => {
  prepare(t)

  const { status, stdout } = execPnpmSync('add')

  t.equal(status, 1)
  t.ok(stdout.toString().includes('`pnpm add` requires the package name'))

  t.end()
})

test('`pnpm recursive add` should fail if no package name was provided', (t: tape.Test) => {
  prepare(t)

  const { status, stdout } = execPnpmSync('recursive', 'add')

  t.equal(status, 1)
  t.ok(stdout.toString().includes('`pnpm recursive add` requires the package name'))

  t.end()
})

test('install should fail if the used pnpm version does not satisfy the pnpm version specified in engines', async (t: tape.Test) => {
  prepare(t, {
    name: 'project',
    version: '1.0.0',

    engines: {
      pnpm: '99999',
    },
  })

  const { status, stdout } = execPnpmSync('install')

  t.equal(status, 1)
  t.ok(stdout.toString().includes('Your pnpm version is incompatible with'))
})

test('engine-strict=false: install should not fail if the used Node version does not satisfy the Node version specified in engines', async (t: tape.Test) => {
  prepare(t, {
    name: 'project',
    version: '1.0.0',

    engines: {
      node: '99999',
    },
  })

  const { status, stdout } = execPnpmSync('install')

  t.equal(status, 0)
  t.ok(stdout.toString().includes('Unsupported engine'))
})

test('engine-strict=true: install should fail if the used Node version does not satisfy the Node version specified in engines', async (t: tape.Test) => {
  prepare(t, {
    name: 'project',
    version: '1.0.0',

    engines: {
      node: '99999',
    },
  })

  const { status, stdout } = execPnpmSync('install', '--engine-strict')

  t.equal(status, 1)
  t.ok(stdout.toString().includes('Your Node version is incompatible with'))
})

test('recursive install should fail if the used pnpm version does not satisfy the pnpm version specified in engines of any of the workspace packages', async (t: tape.Test) => {
  preparePackages(t, [
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

  const { status, stdout } = execPnpmSync('recursive', 'install')

  t.equal(status, 1)
  t.ok(stdout.toString().includes('Your pnpm version is incompatible with'))
})

test('engine-strict=true: recursive install should fail if the used Node version does not satisfy the Node version specified in engines of any of the workspace packages', async (t: tape.Test) => {
  preparePackages(t, [
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

  const { status, stdout } = execPnpmSync('recursive', 'install', '--engine-strict')

  t.equal(status, 1)
  t.ok(stdout.toString().includes('Your Node version is incompatible with'))
})

test('engine-strict=false: recursive install should not fail if the used Node version does not satisfy the Node version specified in engines of any of the workspace packages', async (t: tape.Test) => {
  preparePackages(t, [
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

  const { status, stdout } = execPnpmSync('recursive', 'install')

  t.equal(status, 0)
  t.ok(stdout.toString().includes('Unsupported engine'))
})

test('using a custom virtual-store-dir location', async (t: tape.Test) => {
  prepare(t, {
    dependencies: { 'rimraf': '2.5.1' }
  })

  await execPnpm('install', '--virtual-store-dir=.pnpm')

  t.ok(await exists('.pnpm/localhost+4873/rimraf/2.5.1/node_modules/rimraf/package.json'))
  t.ok(await exists('.pnpm/lock.yaml'))
  t.ok(await exists('.pnpm/node_modules/once/package.json'))

  await rimraf('node_modules')
  await rimraf('.pnpm')

  await execPnpm('install', '--virtual-store-dir=.pnpm', '--frozen-lockfile')

  t.ok(await exists('.pnpm/localhost+4873/rimraf/2.5.1/node_modules/rimraf/package.json'))
  t.ok(await exists('.pnpm/lock.yaml'))
  t.ok(await exists('.pnpm/node_modules/once/package.json'))
})
