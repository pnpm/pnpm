/// <reference path="../../../typings/local.d.ts"/>
import { LOCKFILE_VERSION, WANTED_LOCKFILE } from '@pnpm/constants'
import {
  pruneLockfile,
  pruneSharedLockfile,
} from '@pnpm/prune-lockfile'
import test = require('tape')
import yaml = require('yaml-tag')

const DEFAULT_OPTS = {
  warn (msg: string) {
    // ignore
  },
}

test('remove one redundant package', t => {
  t.deepEqual(pruneLockfile({
    importers: {
      '.': {
        dependencies: {
          'is-positive': '1.0.0',
        },
        specifiers: {
          'is-positive': '^1.0.0',
        },
      },
    },
    lockfileVersion: LOCKFILE_VERSION,
    packages: {
      '/is-positive/1.0.0': {
        dev: false,
        resolution: {
          integrity: 'sha1-ChbBDewTLAqLCzb793Fo5VDvg/g=',
        },
      },
      '/is-positive/2.0.0': {
        dev: false,
        resolution: {
          integrity: 'sha1-ChbBDewTLAqLCzb793Fo5VDvg/g=',
        },
      },
    },
  }, {
    name: 'foo',
    version: '1.0.0',

    dependencies: {
      'is-positive': '^1.0.0',
    },
  }, '.', DEFAULT_OPTS), {
    importers: {
      '.': {
        dependencies: {
          'is-positive': '1.0.0',
        },
        specifiers: {
          'is-positive': '^1.0.0',
        },
      },
    },
    lockfileVersion: LOCKFILE_VERSION,
    packages: {
      '/is-positive/1.0.0': {
        dev: false,
        resolution: {
          integrity: 'sha1-ChbBDewTLAqLCzb793Fo5VDvg/g=',
        },
      },
    },
  })

  t.end()
})

test('remove redundant linked package', t => {
  t.deepEqual(pruneLockfile({
    importers: {
      '.': {
        dependencies: {
          'is-positive': 'link:../is-positive',
        },
        specifiers: {
          'is-positive': '^1.0.0',
        },
      },
    },
    lockfileVersion: LOCKFILE_VERSION,
    packages: {},
  }, {
    name: 'foo',
    version: '1.0.0',

    dependencies: {},
  }, '.', DEFAULT_OPTS), {
    importers: {
      '.': {
        specifiers: {},
      },
    },
    lockfileVersion: LOCKFILE_VERSION,
  })

  t.end()
})

test('keep all', t => {
  t.deepEqual(pruneLockfile({
    importers: {
      '.': {
        dependencies: {
          'is-negative': '1.0.0',
          'is-positive': '1.0.0',
        },
        specifiers: {
          'is-negative': '^1.0.0',
          'is-positive': '^1.0.0',
        },
      },
    },
    lockfileVersion: LOCKFILE_VERSION,
    packages: {
      '/is-negative/1.0.0': {
        dependencies: {
          'is-positive': '2.0.0',
        },
        dev: false,
        resolution: {
          integrity: 'sha1-ChbBDewTLAqLCzb793Fo5VDvg/g=',
        },
      },
      '/is-positive/1.0.0': {
        dev: false,
        resolution: {
          integrity: 'sha1-ChbBDewTLAqLCzb793Fo5VDvg/g=',
        },
      },
      '/is-positive/2.0.0': {
        dev: false,
        resolution: {
          integrity: 'sha1-ChbBDewTLAqLCzb793Fo5VDvg/g=',
        },
      },
    },
  }, {
    name: 'foo',
    version: '1.0.0',

    dependencies: {
      'is-negative': '^1.0.0',
      'is-positive': '^1.0.0',
    },
  }, '.', DEFAULT_OPTS), {
    importers: {
      '.': {
        dependencies: {
          'is-negative': '1.0.0',
          'is-positive': '1.0.0',
        },
        specifiers: {
          'is-negative': '^1.0.0',
          'is-positive': '^1.0.0',
        },
      },
    },
    lockfileVersion: LOCKFILE_VERSION,
    packages: {
      '/is-negative/1.0.0': {
        dependencies: {
          'is-positive': '2.0.0',
        },
        dev: false,
        resolution: {
          integrity: 'sha1-ChbBDewTLAqLCzb793Fo5VDvg/g=',
        },
      },
      '/is-positive/1.0.0': {
        dev: false,
        resolution: {
          integrity: 'sha1-ChbBDewTLAqLCzb793Fo5VDvg/g=',
        },
      },
      '/is-positive/2.0.0': {
        dev: false,
        resolution: {
          integrity: 'sha1-ChbBDewTLAqLCzb793Fo5VDvg/g=',
        },
      },
    },
  })

  t.end()
})

test('optional dependency should have optional = true', t => {
  t.deepEqual(pruneLockfile({
    importers: {
      '.': {
        dependencies: {
          'parent-of-foo': '1.0.0',
          'pkg-with-good-optional': '1.0.0',
        },
        optionalDependencies: {
          'is-positive': '1.0.0',
        },
        specifiers: {
          'is-positive': '^1.0.0',
          'parent-of-foo': '1.0.0',
          'pkg-with-good-optional': '^1.0.0',
        },
      },
    },
    lockfileVersion: LOCKFILE_VERSION,
    packages: {
      '/foo-child/1.0.0': {
        optional: true,
        resolution: {
          integrity: 'sha1-ChbBDewTLAqLCzb793Fo5VDvg/g=',
        },
      },
      '/foo/1.0.0': {
        dependencies: {
          'foo-child': '1.0.0',
        },
        optional: true,
        resolution: {
          integrity: 'sha1-ChbBDewTLAqLCzb793Fo5VDvg/g=',
        },
      },
      '/is-positive/1.0.0': {
        dev: false,
        resolution: {
          integrity: 'sha1-ChbBDewTLAqLCzb793Fo5VDvg/g=',
        },
      },
      '/parent-of-foo/1.0.0': {
        dependencies: {
          foo: '1.0.0',
        },
        dev: false,
        resolution: {
          integrity: 'sha1-ChbBDewTLAqLCzb793Fo5VDvg/g=',
        },
      },
      '/pkg-with-good-optional/1.0.0': {
        dev: false,
        optionalDependencies: {
          foo: '1.0.0',
          'is-positive': '1.0.0',
        },
        resolution: {
          integrity: 'sha1-ChbBDewTLAqLCzb793Fo5VDvg/g=',
        },
      },
    },
  }, {
    name: 'foo',
    version: '1.0.0',

    dependencies: {
      'parent-of-foo': '1.0.0',
      'pkg-with-good-optional': '^1.0.0',
    },
    optionalDependencies: {
      'is-positive': '^1.0.0',
    },
  }, '.', DEFAULT_OPTS), {
    importers: {
      '.': {
        dependencies: {
          'parent-of-foo': '1.0.0',
          'pkg-with-good-optional': '1.0.0',
        },
        optionalDependencies: {
          'is-positive': '1.0.0',
        },
        specifiers: {
          'is-positive': '^1.0.0',
          'parent-of-foo': '1.0.0',
          'pkg-with-good-optional': '^1.0.0',
        },
      },
    },
    lockfileVersion: LOCKFILE_VERSION,
    packages: {
      '/foo-child/1.0.0': {
        dev: false,
        resolution: {
          integrity: 'sha1-ChbBDewTLAqLCzb793Fo5VDvg/g=',
        },
      },
      '/foo/1.0.0': {
        dependencies: {
          'foo-child': '1.0.0',
        },
        dev: false,
        resolution: {
          integrity: 'sha1-ChbBDewTLAqLCzb793Fo5VDvg/g=',
        },
      },
      '/is-positive/1.0.0': {
        dev: false,
        optional: true,
        resolution: {
          integrity: 'sha1-ChbBDewTLAqLCzb793Fo5VDvg/g=',
        },
      },
      '/parent-of-foo/1.0.0': {
        dependencies: {
          foo: '1.0.0',
        },
        dev: false,
        resolution: {
          integrity: 'sha1-ChbBDewTLAqLCzb793Fo5VDvg/g=',
        },
      },
      '/pkg-with-good-optional/1.0.0': {
        dev: false,
        optionalDependencies: {
          foo: '1.0.0',
          'is-positive': '1.0.0',
        },
        resolution: {
          integrity: 'sha1-ChbBDewTLAqLCzb793Fo5VDvg/g=',
        },
      },
    },
  })

  t.end()
})

test('optional dependency should not have optional = true if used not only as optional', t => {
  t.deepEqual(pruneLockfile({
    importers: {
      '.': {
        dependencies: {
          'is-positive': '1.0.0',
          'pkg-with-good-optional': '1.0.0',
        },
        specifiers: {
          'is-positive': '^1.0.0',
          'pkg-with-good-optional': '^1.0.0',
        },
      },
    },
    lockfileVersion: LOCKFILE_VERSION,
    packages: {
      '/is-positive/1.0.0': {
        dev: false,
        resolution: {
          integrity: 'sha1-ChbBDewTLAqLCzb793Fo5VDvg/g=',
        },
      },
      '/pkg-with-good-optional/1.0.0': {
        dev: false,
        optionalDependencies: {
          'is-positive': '1.0.0',
        },
        resolution: {
          integrity: 'sha1-ChbBDewTLAqLCzb793Fo5VDvg/g=',
        },
      },
    },
  }, {
    name: 'foo',
    version: '1.0.0',

    dependencies: {
      'is-positive': '^1.0.0',
      'pkg-with-good-optional': '^1.0.0',
    },
  }, '.', DEFAULT_OPTS), {
    importers: {
      '.': {
        dependencies: {
          'is-positive': '1.0.0',
          'pkg-with-good-optional': '1.0.0',
        },
        specifiers: {
          'is-positive': '^1.0.0',
          'pkg-with-good-optional': '^1.0.0',
        },
      },
    },
    lockfileVersion: LOCKFILE_VERSION,
    packages: {
      '/is-positive/1.0.0': {
        dev: false,
        resolution: {
          integrity: 'sha1-ChbBDewTLAqLCzb793Fo5VDvg/g=',
        },
      },
      '/pkg-with-good-optional/1.0.0': {
        dev: false,
        optionalDependencies: {
          'is-positive': '1.0.0',
        },
        resolution: {
          integrity: 'sha1-ChbBDewTLAqLCzb793Fo5VDvg/g=',
        },
      },
    },
  })

  t.end()
})

test('dev dependency should have dev = true', t => {
  t.deepEqual(pruneLockfile({
    importers: {
      '.': {
        dependencies: {
          'pkg-with-good-optional': '1.0.0',
        },
        devDependencies: {
          'is-positive': '1.0.0',
        },
        specifiers: {
          'is-positive': '^1.0.0',
          'pkg-with-good-optional': '^1.0.0',
        },
      },
    },
    lockfileVersion: LOCKFILE_VERSION,
    packages: {
      '/is-positive/1.0.0': {
        resolution: {
          integrity: 'sha1-ChbBDewTLAqLCzb793Fo5VDvg/g=',
        },
      },
      '/pkg-with-good-optional/1.0.0': {
        dev: false,
        resolution: {
          integrity: 'sha1-ChbBDewTLAqLCzb793Fo5VDvg/g=',
        },
      },
    },
  }, {
    name: 'foo',
    version: '1.0.0',

    dependencies: {
      'pkg-with-good-optional': '^1.0.0',
    },
    devDependencies: {
      'is-positive': '^1.0.0',
    },
  }, '.', DEFAULT_OPTS), {
    importers: {
      '.': {
        dependencies: {
          'pkg-with-good-optional': '1.0.0',
        },
        devDependencies: {
          'is-positive': '1.0.0',
        },
        specifiers: {
          'is-positive': '^1.0.0',
          'pkg-with-good-optional': '^1.0.0',
        },
      },
    },
    lockfileVersion: LOCKFILE_VERSION,
    packages: {
      '/is-positive/1.0.0': {
        dev: true,
        resolution: {
          integrity: 'sha1-ChbBDewTLAqLCzb793Fo5VDvg/g=',
        },
      },
      '/pkg-with-good-optional/1.0.0': {
        dev: false,
        resolution: {
          integrity: 'sha1-ChbBDewTLAqLCzb793Fo5VDvg/g=',
        },
      },
    },
  })

  t.end()
})

test('dev dependency should not have dev = true if it is used not only as dev', t => {
  t.deepEqual(pruneLockfile({
    importers: {
      '.': {
        dependencies: {
          'some-pkg': '1.0.0',
        },
        devDependencies: {
          'is-positive': '1.0.0',
        },
        specifiers: {
          'is-positive': '^1.0.0',
          'some-pkg': '^1.0.0',
        },
      },
    },
    lockfileVersion: LOCKFILE_VERSION,
    packages: {
      '/is-positive/1.0.0': {
        resolution: {
          integrity: 'sha1-ChbBDewTLAqLCzb793Fo5VDvg/g=',
        },
      },
      '/some-pkg/1.0.0': {
        dependencies: {
          'is-positive': '1.0.0',
        },
        dev: false,
        resolution: {
          integrity: 'sha1-ChbBDewTLAqLCzb793Fo5VDvg/g=',
        },
      },
    },
  }, {
    name: 'foo',
    version: '1.0.0',

    dependencies: {
      'some-pkg': '^1.0.0',
    },
    devDependencies: {
      'is-positive': '^1.0.0',
    },
  }, '.', DEFAULT_OPTS), {
    importers: {
      '.': {
        dependencies: {
          'some-pkg': '1.0.0',
        },
        devDependencies: {
          'is-positive': '1.0.0',
        },
        specifiers: {
          'is-positive': '^1.0.0',
          'some-pkg': '^1.0.0',
        },
      },
    },
    lockfileVersion: LOCKFILE_VERSION,
    packages: {
      '/is-positive/1.0.0': {
        resolution: {
          integrity: 'sha1-ChbBDewTLAqLCzb793Fo5VDvg/g=',
        },
      },
      '/some-pkg/1.0.0': {
        dependencies: {
          'is-positive': '1.0.0',
        },
        dev: false,
        resolution: {
          integrity: 'sha1-ChbBDewTLAqLCzb793Fo5VDvg/g=',
        },
      },
    },
  })

  t.end()
})

test('the dev field should be updated to dev = false if it is not a dev dependency anymore', t => {
  t.deepEqual(pruneLockfile({
    importers: {
      '.': {
        dependencies: {
          a: '1.0.0',
        },
        specifiers: {
          a: '^1.0.0',
        },
      },
    },
    lockfileVersion: LOCKFILE_VERSION,
    packages: {
      '/a/1.0.0': {
        resolution: {
          integrity: 'sha1-ChbBDewTLAqLCzb793Fo5VDvg/g=',
        },
      },
    },
  }, {
    name: 'foo',
    version: '1.0.0',

    dependencies: {
      a: '^1.0.0',
    },
  }, '.', DEFAULT_OPTS), {
    importers: {
      '.': {
        dependencies: {
          a: '1.0.0',
        },
        specifiers: {
          a: '^1.0.0',
        },
      },
    },
    lockfileVersion: LOCKFILE_VERSION,
    packages: {
      '/a/1.0.0': {
        dev: false,
        resolution: {
          integrity: 'sha1-ChbBDewTLAqLCzb793Fo5VDvg/g=',
        },
      },
    },
  })

  t.end()
})

test('subdependency is both optional and dev', t => {
  t.deepEqual(pruneLockfile(yaml`
    importers:
      .:
        dependencies:
          prod-parent: 1.0.0
        devDependencies:
          parent: 1.0.0
        specifiers:
          parent: ^1.0.0
          prod-parent: ^1.0.0
    lockfileVersion: 5
    packages:
      /parent/1.0.0:
        optionalDependencies:
          subdep: 1.0.0
          subdep2: 1.0.0
        resolution:
          integrity: sha1-Sb1jMdfQLQwJvJEKEHW6gWW1bfk=
      /prod-parent/1.0.0:
        dependencies:
          subdep2: 1.0.0
        resolution:
          integrity: sha1-Sb1jMdfQLQwJvJEKEHW6gWW1bfk=
      /subdep/1.0.0:
        resolution:
          integrity: sha1-Sb1jMdfQLQwJvJEKEHW6gWW1bfk=
      /subdep2/1.0.0:
        resolution:
          integrity: sha1-Sb1jMdfQLQwJvJEKEHW6gWW1bfk=
  `, {
    name: 'foo',
    version: '1.0.0',

    dependencies: {
      'prod-parent': '^1.0.0',
    },
    devDependencies: {
      parent: '^1.0.0',
    },
  }, '.', DEFAULT_OPTS), yaml`
    importers:
      .:
        dependencies:
          prod-parent: 1.0.0
        devDependencies:
          parent: 1.0.0
        specifiers:
          parent: ^1.0.0
          prod-parent: ^1.0.0
    lockfileVersion: 5
    packages:
      /parent/1.0.0:
        dev: true
        optionalDependencies:
          subdep: 1.0.0
          subdep2: 1.0.0
        resolution:
          integrity: sha1-Sb1jMdfQLQwJvJEKEHW6gWW1bfk=
      /prod-parent/1.0.0:
        dev: false
        dependencies:
          subdep2: 1.0.0
        resolution:
          integrity: sha1-Sb1jMdfQLQwJvJEKEHW6gWW1bfk=
      /subdep/1.0.0:
        dev: true
        optional: true
        resolution:
          integrity: sha1-Sb1jMdfQLQwJvJEKEHW6gWW1bfk=
      /subdep2/1.0.0:
        resolution:
          integrity: sha1-Sb1jMdfQLQwJvJEKEHW6gWW1bfk=
  `)

  t.end()
})

test('dev = true is removed if dependency is used both as dev and prod dependency', t => {
  t.deepEqual(pruneLockfile(yaml`
    importers:
      .:
        dependencies:
          foo: /inflight/1.0.6
        devDependencies:
          inflight: 1.0.6
        specifiers:
          foo: 'npm:inflight@^1.0.6'
          inflight: ^1.0.6
    lockfileVersion: 5
    packages:
      /inflight/1.0.6:
        dev: true
        dependencies:
          once: 1.4.0
          wrappy: 1.0.2
        resolution:
          integrity: sha1-Sb1jMdfQLQwJvJEKEHW6gWW1bfk=
      /once/1.4.0:
        dev: true
        dependencies:
          wrappy: 1.0.2
        resolution:
          integrity: sha1-WDsap3WWHUsROsF9nFC6753Xa9E=
      /wrappy/1.0.2:
        dev: true
        resolution:
          integrity: sha1-tSQ9jz7BqjXxNkYFvA0QNuMKtp8=
  `, {
    name: 'foo',
    version: '1.0.0',

    dependencies: {
      foo: 'npm:inflight@^1.0.6',
    },
    devDependencies: {
      inflight: '^1.0.6',
    },
  }, '.', DEFAULT_OPTS), yaml`
    importers:
      .:
        dependencies:
          foo: /inflight/1.0.6
        devDependencies:
          inflight: 1.0.6
        specifiers:
          foo: 'npm:inflight@^1.0.6'
          inflight: ^1.0.6
    lockfileVersion: 5
    packages:
      /inflight/1.0.6:
        dependencies:
          once: 1.4.0
          wrappy: 1.0.2
        resolution:
          integrity: sha1-Sb1jMdfQLQwJvJEKEHW6gWW1bfk=
      /once/1.4.0:
        dependencies:
          wrappy: 1.0.2
        resolution:
          integrity: sha1-WDsap3WWHUsROsF9nFC6753Xa9E=
      /wrappy/1.0.2:
        resolution:
          integrity: sha1-tSQ9jz7BqjXxNkYFvA0QNuMKtp8=
  `)

  t.end()
})

test('optional = true is removed if dependency is used both as optional and prod dependency', t => {
  t.deepEqual(pruneLockfile(yaml`
    importers:
      .:
        dependencies:
          foo: /inflight/1.0.6
        optionalDependencies:
          inflight: 1.0.6
        specifiers:
          foo: 'npm:inflight@^1.0.6'
          inflight: ^1.0.6
    lockfileVersion: 5
    packages:
      /inflight/1.0.6:
        optional: true
        dependencies:
          once: 1.4.0
          wrappy: 1.0.2
        resolution:
          integrity: sha1-Sb1jMdfQLQwJvJEKEHW6gWW1bfk=
      /once/1.4.0:
        optional: true
        dependencies:
          wrappy: 1.0.2
        resolution:
          integrity: sha1-WDsap3WWHUsROsF9nFC6753Xa9E=
      /wrappy/1.0.2:
        optional: true
        resolution:
          integrity: sha1-tSQ9jz7BqjXxNkYFvA0QNuMKtp8=
  `, {
    name: 'foo',
    version: '1.0.0',

    dependencies: {
      foo: 'npm:inflight@^1.0.6',
    },
    optionalDependencies: {
      inflight: '^1.0.6',
    },
  }, '.', DEFAULT_OPTS), yaml`
    importers:
      .:
        dependencies:
          foo: /inflight/1.0.6
        optionalDependencies:
          inflight: 1.0.6
        specifiers:
          foo: 'npm:inflight@^1.0.6'
          inflight: ^1.0.6
    lockfileVersion: 5
    packages:
      /inflight/1.0.6:
        dev: false
        dependencies:
          once: 1.4.0
          wrappy: 1.0.2
        resolution:
          integrity: sha1-Sb1jMdfQLQwJvJEKEHW6gWW1bfk=
      /once/1.4.0:
        dev: false
        dependencies:
          wrappy: 1.0.2
        resolution:
          integrity: sha1-WDsap3WWHUsROsF9nFC6753Xa9E=
      /wrappy/1.0.2:
        dev: false
        resolution:
          integrity: sha1-tSQ9jz7BqjXxNkYFvA0QNuMKtp8=
  `)

  t.end()
})

test('remove dependencies that are not in the package', t => {
  t.deepEqual(pruneLockfile({
    importers: {
      '.': {
        dependencies: {
          'is-positive': '1.0.0',
        },
        devDependencies: {
          'is-negative': '1.0.0',
        },
        optionalDependencies: {
          fsevents: '1.0.0',
        },
        specifiers: {
          fsevents: '^1.0.0',
          'is-negative': '^1.0.0',
          'is-positive': '^1.0.0',
        },
      },
    },
    lockfileVersion: LOCKFILE_VERSION,
    packages: {
      '/fsevents/1.0.0': {
        resolution: {
          integrity: 'sha1-ChbBDewTLAqLCzb793Fo5VDvg/g=',
        },
      },
      '/is-negative/1.0.0': {
        resolution: {
          integrity: 'sha1-ChbBDewTLAqLCzb793Fo5VDvg/g=',
        },
      },
      '/is-positive/1.0.0': {
        resolution: {
          integrity: 'sha1-ChbBDewTLAqLCzb793Fo5VDvg/g=',
        },
      },
    },
  }, {
    name: 'foo',
    version: '1.0.0',
  }, '.', DEFAULT_OPTS), {
    importers: {
      '.': {
        specifiers: {},
      },
    },
    lockfileVersion: LOCKFILE_VERSION,
  })

  t.end()
})

test(`ignore dependencies that are in package.json but are not in ${WANTED_LOCKFILE}`, t => {
  t.deepEqual(pruneLockfile({
    importers: {
      '.': {
        dependencies: {
          'is-positive': '1.0.0',
        },
        specifiers: {
          'is-positive': '^1.0.0',
        },
      },
    },
    lockfileVersion: LOCKFILE_VERSION,
    packages: {
      '/is-positive/1.0.0': {
        dev: false,
        resolution: {
          integrity: 'sha1-ChbBDewTLAqLCzb793Fo5VDvg/g=',
        },
      },
    },
  }, {
    name: 'foo',
    version: '1.0.0',

    dependencies: {
      'is-negative': '^1.0.0',
      'is-positive': '^1.0.0',
    },
  }, '.', DEFAULT_OPTS), {
    importers: {
      '.': {
        dependencies: {
          'is-positive': '1.0.0',
        },
        specifiers: {
          'is-positive': '^1.0.0',
        },
      },
    },
    lockfileVersion: LOCKFILE_VERSION,
    packages: {
      '/is-positive/1.0.0': {
        dev: false,
        resolution: {
          integrity: 'sha1-ChbBDewTLAqLCzb793Fo5VDvg/g=',
        },
      },
    },
  })

  t.end()
})

// this test may be redundant
test('keep lockfileMinorVersion, if present', t => {
  t.deepEqual(pruneLockfile({
    importers: {
      '.': {
        dependencies: {
          'is-positive': '1.0.0',
        },
        specifiers: {
          'is-positive': '^1.0.0',
        },
      },
    },
    lockfileVersion: 5.2,
    packages: {
      '/is-positive/1.0.0': {
        dev: false,
        resolution: {
          integrity: 'sha1-ChbBDewTLAqLCzb793Fo5VDvg/g=',
        },
      },
    },
  }, {
    name: 'foo',
    version: '1.0.0',

    dependencies: {
      'is-positive': '^1.0.0',
    },
  }, '.', DEFAULT_OPTS), {
    importers: {
      '.': {
        dependencies: {
          'is-positive': '1.0.0',
        },
        specifiers: {
          'is-positive': '^1.0.0',
        },
      },
    },
    lockfileVersion: 5.2,
    packages: {
      '/is-positive/1.0.0': {
        dev: false,
        resolution: {
          integrity: 'sha1-ChbBDewTLAqLCzb793Fo5VDvg/g=',
        },
      },
    },
  })

  t.end()
})

test('keep linked package even if it is not in package.json', t => {
  t.deepEqual(pruneLockfile({
    importers: {
      '.': {
        dependencies: {
          'is-negative': '1.0.0',
          'is-positive': 'link:../is-positive',
        },
        specifiers: {
          'is-negative': '^1.0.0',
        },
      },
    },
    lockfileVersion: LOCKFILE_VERSION,
    packages: {
      '/is-negative/1.0.0': {
        dev: false,
        resolution: {
          integrity: 'sha1-ChbBDewTLAqLCzb793Fo5VDvg/g=',
        },
      },
    },
  }, {
    name: 'foo',
    version: '1.0.0',

    dependencies: {
      'is-negative': '^1.0.0',
    },
  }, '.', DEFAULT_OPTS), {
    importers: {
      '.': {
        dependencies: {
          'is-negative': '1.0.0',
          'is-positive': 'link:../is-positive',
        },
        specifiers: {
          'is-negative': '^1.0.0',
        },
      },
    },
    lockfileVersion: LOCKFILE_VERSION,
    packages: {
      '/is-negative/1.0.0': {
        dev: false,
        resolution: {
          integrity: 'sha1-ChbBDewTLAqLCzb793Fo5VDvg/g=',
        },
      },
    },
  })

  t.end()
})

test("prune: don't remove package used by another importer", t => {
  t.deepEqual(pruneLockfile({
    importers: {
      'packages/package-1': {
        dependencies: {
          'is-positive': '1.0.0',
        },
        specifiers: {
          'is-positive': '^1.0.0',
        },
      },
      'packages/package-2': {
        dependencies: {
          'is-negative': '1.0.0',
        },
        specifiers: {
          'is-negative': '^1.0.0',
        },
      },
    },
    lockfileVersion: LOCKFILE_VERSION,
    packages: {
      '/is-negative/1.0.0': {
        dev: false,
        resolution: {
          integrity: 'sha1-ChbBDewTLAqLCzb793Fo5VDvg/g=',
        },
      },
      '/is-positive/1.0.0': {
        dev: false,
        resolution: {
          integrity: 'sha1-ChbBDewTLAqLCzb793Fo5VDvg/g=',
        },
      },
      '/is-positive/2.0.0': {
        dev: false,
        resolution: {
          integrity: 'sha1-ChbBDewTLAqLCzb793Fo5VDvg/g=',
        },
      },
    },
  }, {
    name: 'project-2',
    version: '1.0.0',

    dependencies: { 'is-negative': '^1.0.0' },
  }, 'packages/package-2', DEFAULT_OPTS), {
    importers: {
      'packages/package-1': {
        dependencies: {
          'is-positive': '1.0.0',
        },
        specifiers: {
          'is-positive': '^1.0.0',
        },
      },
      'packages/package-2': {
        dependencies: {
          'is-negative': '1.0.0',
        },
        specifiers: {
          'is-negative': '^1.0.0',
        },
      },
    },
    lockfileVersion: LOCKFILE_VERSION,
    packages: {
      '/is-negative/1.0.0': {
        dev: false,
        resolution: {
          integrity: 'sha1-ChbBDewTLAqLCzb793Fo5VDvg/g=',
        },
      },
      '/is-positive/1.0.0': {
        dev: false,
        resolution: {
          integrity: 'sha1-ChbBDewTLAqLCzb793Fo5VDvg/g=',
        },
      },
    },
  })

  t.end()
})

test('pruneSharedLockfile: remove one redundant package', t => {
  t.deepEqual(pruneSharedLockfile({
    importers: {
      'packages/package-1': {
        dependencies: {
          'is-positive': '1.0.0',
        },
        specifiers: {
          'is-positive': '^1.0.0',
        },
      },
    },
    lockfileVersion: LOCKFILE_VERSION,
    packages: {
      '/is-positive/1.0.0': {
        dev: false,
        resolution: {
          integrity: 'sha1-ChbBDewTLAqLCzb793Fo5VDvg/g=',
        },
      },
      '/is-positive/2.0.0': {
        dev: false,
        resolution: {
          integrity: 'sha1-ChbBDewTLAqLCzb793Fo5VDvg/g=',
        },
      },
    },
  }, DEFAULT_OPTS), {
    importers: {
      'packages/package-1': {
        dependencies: {
          'is-positive': '1.0.0',
        },
        specifiers: {
          'is-positive': '^1.0.0',
        },
      },
    },
    lockfileVersion: LOCKFILE_VERSION,
    packages: {
      '/is-positive/1.0.0': {
        dev: false,
        resolution: {
          integrity: 'sha1-ChbBDewTLAqLCzb793Fo5VDvg/g=',
        },
      },
    },
  })

  t.end()
})
