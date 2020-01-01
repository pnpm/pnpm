import { satisfiesPackageManifest } from '@pnpm/lockfile-utils'
import test = require('tape')

const DEFAULT_LOCKFILE_FIELDS = {
  lockfileVersion: 3,
}

const DEFAULT_PKG_FIELDS = {
  name: 'project',
  version: '1.0.0',
}

test('satisfiesPackageManifest()', t => {
  t.ok(satisfiesPackageManifest({
    ...DEFAULT_LOCKFILE_FIELDS,
    importers: {
      '.': {
        dependencies: { foo: '1.0.0' },
        specifiers: { foo: '^1.0.0' },
      },
    },
  }, {
    ...DEFAULT_PKG_FIELDS,
    dependencies: { foo: '^1.0.0' },
  }, '.'))
  t.ok(satisfiesPackageManifest({
    ...DEFAULT_LOCKFILE_FIELDS,
    importers: {
      '.': {
        dependencies: { foo: '1.0.0' },
        devDependencies: {},
        specifiers: { foo: '^1.0.0' },
      },
    },
  }, {
    ...DEFAULT_PKG_FIELDS,
    dependencies: { foo: '^1.0.0' },
  }, '.'))
  t.ok(satisfiesPackageManifest({
    ...DEFAULT_LOCKFILE_FIELDS,
    importers: {
      '.': {
        devDependencies: { foo: '1.0.0' },
        specifiers: { foo: '^1.0.0' },
      },
    },
  }, {
    ...DEFAULT_PKG_FIELDS,
    devDependencies: { foo: '^1.0.0' },
  }, '.'))
  t.ok(satisfiesPackageManifest({
    ...DEFAULT_LOCKFILE_FIELDS,
    importers: {
      '.': {
        optionalDependencies: { foo: '1.0.0' },
        specifiers: { foo: '^1.0.0' },
      },
    },
  }, {
    ...DEFAULT_PKG_FIELDS,
    optionalDependencies: { foo: '^1.0.0' },
  }, '.'))
  t.notOk(satisfiesPackageManifest({
    ...DEFAULT_LOCKFILE_FIELDS,
    importers: {
      '.': {
        dependencies: { foo: '1.0.0' },
        specifiers: { foo: '^1.0.0' },
      },
    },
  }, {
    ...DEFAULT_PKG_FIELDS,
    optionalDependencies: { foo: '^1.0.0' },
  }, '.'), 'dep type differs')
  t.notOk(satisfiesPackageManifest({
    ...DEFAULT_LOCKFILE_FIELDS,
    importers: {
      '.': {
        dependencies: { foo: '1.0.0' },
        specifiers: { foo: '^1.0.0' },
      },
    },
  }, {
    ...DEFAULT_PKG_FIELDS,
    dependencies: { foo: '^1.1.0' },
  }, '.'), 'spec does not match')
  t.notOk(satisfiesPackageManifest({
    ...DEFAULT_LOCKFILE_FIELDS,
    importers: {
      '.': {
        dependencies: { foo: '1.0.0' },
        specifiers: { foo: '^1.0.0' },
      },
    },
  }, {
    ...DEFAULT_PKG_FIELDS,
    dependencies: { foo: '^1.0.0', bar: '2.0.0' },
  }, '.'), 'dep spec missing')
  t.notOk(satisfiesPackageManifest({
    ...DEFAULT_LOCKFILE_FIELDS,
    importers: {
      '.': {
        dependencies: { foo: '1.0.0' },
        specifiers: { foo: '^1.0.0', bar: '2.0.0' },
      },
    },
  }, {
    ...DEFAULT_PKG_FIELDS,
    dependencies: { foo: '^1.0.0', bar: '2.0.0' },
  }, '.'))

  {
    const lockfile = {
      ...DEFAULT_LOCKFILE_FIELDS,
      importers: {
        '.': {
          dependencies: {
            foo: '1.0.0',
          },
          optionalDependencies: {
            bar: '2.0.0',
          },
          specifiers: {
            bar: '2.0.0',
            foo: '^1.0.0',
          },
        },
      },
    }
    const pkg = {
      ...DEFAULT_PKG_FIELDS,
      dependencies: {
        bar: '2.0.0',
        foo: '^1.0.0',
      },
      optionalDependencies: {
        bar: '2.0.0',
      },
    }
    t.ok(satisfiesPackageManifest(lockfile, pkg, '.'))
  }

  {
    const lockfile = {
      ...DEFAULT_LOCKFILE_FIELDS,
      importers: {
        '.': {
          dependencies: {
            bar: '2.0.0',
            qar: '1.0.0',
          },
          specifiers: {
            bar: '2.0.0',
            qar: '^1.0.0',
          },
        },
      },
    }
    const pkg = {
      ...DEFAULT_PKG_FIELDS,
      dependencies: {
        bar: '2.0.0',
      },
    }
    t.notOk(satisfiesPackageManifest(lockfile, pkg, '.'))
  }

  {
    const lockfile = {
      ...DEFAULT_LOCKFILE_FIELDS,
      importers: {
        '.': {
          dependencies: {
            bar: '2.0.0',
            qar: '1.0.0',
          },
          specifiers: {
            bar: '2.0.0',
          },
        },
      },
    }
    const pkg = {
      ...DEFAULT_PKG_FIELDS,
      dependencies: {
        bar: '2.0.0',
      },
    }
    t.notOk(satisfiesPackageManifest(lockfile, pkg, '.'))
  }

  t.ok(satisfiesPackageManifest({
    ...DEFAULT_LOCKFILE_FIELDS,
    importers: {
      '.': {
        dependencies: { foo: '1.0.0', linked: 'link:../linked' },
        specifiers: { foo: '^1.0.0' },
      },
    },
  }, {
    ...DEFAULT_PKG_FIELDS,
    dependencies: { foo: '^1.0.0' },
  }, '.'), 'linked packages that are not in package.json are ignored')

  t.notOk(satisfiesPackageManifest({
    ...DEFAULT_LOCKFILE_FIELDS,
    importers: {
      'packages/foo': {
        dependencies: { foo: '1.0.0' },
        specifiers: { foo: '^1.0.0' },
      },
    },
  }, {
    ...DEFAULT_PKG_FIELDS,
    dependencies: { foo: '^1.0.0' },
  }, '.'))

  t.ok(satisfiesPackageManifest({
    ...DEFAULT_LOCKFILE_FIELDS,
    importers: {
      '.': {
        dependencies: {
          foo: '1.0.0',
        },
        specifiers: {
          foo: '1.0.0',
        },
      },
    },
  }, {
    ...DEFAULT_PKG_FIELDS,
    dependencies: {
      foo: '1.0.0',
    },
    devDependencies: {
      foo: '1.0.0',
    },
  }, '.'))

  t.end()
})
