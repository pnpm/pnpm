import test = require('tape')
import runLifecycleHook, {runPostinstallHooks} from '@pnpm/lifecycle'
import path = require('path')
import rimraf = require('rimraf')
import loadJsonFile = require('load-json-file')

const fixtures = path.join(__dirname, 'fixtures')
const rootNodeModulesDir = path.join(__dirname, '..', 'node_modules')

test('runLifecycleHook()', async (t) => {
  const pkgRoot = path.join(fixtures, 'simple')
  const pkg = require(path.join(pkgRoot, 'package.json'))
  await runLifecycleHook('postinstall', pkg, {
    rootNodeModulesDir,
    depPath: '/simple/1.0.0',
    rawNpmConfig: {},
    pkgRoot,
    unsafePerm: true,
    userAgent: 'pnpm',
  })

  t.deepEqual(require(path.join(pkgRoot, 'output.json')), ['install'])

  t.end()
})

test('runPostinstallHooks()', async (t) => {
  const pkgRoot = path.join(fixtures, 'with-many-scripts')
  const pkg = require(path.join(pkgRoot, 'package.json'))
  rimraf.sync(path.join(pkgRoot, 'output.json'))
  await runPostinstallHooks({
    rootNodeModulesDir,
    depPath: '/with-many-scripts/1.0.0',
    rawNpmConfig: {},
    pkgRoot,
    unsafePerm: true,
    userAgent: 'pnpm',
  })

  t.deepEqual(loadJsonFile.sync(path.join(pkgRoot, 'output.json')), ['preinstall', 'install', 'postinstall'])

  t.end()
})

test('runPostinstallHooks() with prepare = true', async (t) => {
  const pkgRoot = path.join(fixtures, 'with-many-scripts')
  const pkg = require(path.join(pkgRoot, 'package.json'))
  rimraf.sync(path.join(pkgRoot, 'output.json'))
  await runPostinstallHooks({
    rootNodeModulesDir,
    depPath: '/with-many-scripts/1.0.0',
    rawNpmConfig: {},
    prepare: true,
    pkgRoot,
    unsafePerm: true,
    userAgent: 'pnpm',
  })

  t.deepEqual(loadJsonFile.sync(path.join(pkgRoot, 'output.json')), ['prepare', 'preinstall', 'install', 'postinstall'])

  t.end()
})
