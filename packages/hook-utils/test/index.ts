import pnpmHookUtils = require('@pnpm/hook-utils')
import test = require('tape')

const NOOP_FUNCTION = () => {/* no-op */}

const hooks: Required<pnpmHookUtils.PnpmHooks> = {
  readPackage (pkg, ctx) {
    const utils = pnpmHookUtils.createReadPackageUtils(pkg, ctx)

    switch (pkg.name) {
      case 'stuff':
        utils.setDependencies({
          pnpm: '^3.1.0',
        }, 'dependencies')
    }

    utils.logChanges()
    return pkg
  },
  afterAllResolved: lockfile => lockfile,
}

test('readPackage hook utils work', t => {
  const pkg = hooks.readPackage({
    name: 'stuff',
    version: '0.0.0',

    dependencies: {},
    devDependencies: {},
    optionalDependencies: {},
    peerDependencies: {},
  }, {
    log: NOOP_FUNCTION,
  })
  t.isEqual(pkg.dependencies.pnpm, '^3.1.0')
  t.end()
})
