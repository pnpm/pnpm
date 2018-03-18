import test = require('tape')
import runLifecycleHook, {runPostinstallHooks} from '@pnpm/lifecycle'
import path = require('path')

const fixtures = path.join(__dirname, 'fixtures')
const rootNodeModulesDir = path.join(__dirname, '..', 'node_modules')

test('runLifecycleHook()', async (t) => {
  const pkgRoot = path.join(fixtures, 'simple')
  const pkg = require(path.join(pkgRoot, 'package.json'))
  await runLifecycleHook('postinstall', pkg, {
    rootNodeModulesDir,
    pkgId: '/simple/1.0.0',
    rawNpmConfig: {},
    pkgRoot,
    unsafePerm: true,
    userAgent: 'pnpm',
  })

  t.deepEqual(require(path.join(pkgRoot, 'output.json')), ['install'])

  t.end()
})

test('runLifecycleHook()', async (t) => {
  const pkgRoot = path.join(fixtures, 'with-many-scripts')
  const pkg = require(path.join(pkgRoot, 'package.json'))
  await runPostinstallHooks({
    rootNodeModulesDir,
    pkgId: '/with-many-scripts/1.0.0',
    rawNpmConfig: {},
    pkgRoot,
    unsafePerm: true,
    userAgent: 'pnpm',
  })

  t.deepEqual(require(path.join(pkgRoot, 'output.json')), ['preinstall', 'install', 'postinstall'])

  t.end()
})
