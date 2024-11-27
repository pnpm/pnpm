/// <reference path="../../../__typings__/local.d.ts"/>
import { LOCKFILE_VERSION, WANTED_LOCKFILE } from '@pnpm/constants'
import {
  pruneLockfile,
  pruneSharedLockfile,
} from '@pnpm/lockfile.pruner'
import { type DepPath, type ProjectId } from '@pnpm/types'
import yaml from 'yaml-tag'

const DEFAULT_OPTS = {
  warn (msg: string) {
    // ignore
  },
}

test('remove one redundant package', () => {
  expect(pruneLockfile({
    importers: {
      ['.' as ProjectId]: {
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
      ['is-positive@1.0.0' as DepPath]: {
        resolution: {
          integrity: 'sha1-ChbBDewTLAqLCzb793Fo5VDvg/g=',
        },
      },
      ['is-positive@2.0.0' as DepPath]: {
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
  }, '.' as ProjectId, DEFAULT_OPTS)).toStrictEqual({
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
      'is-positive@1.0.0': {
        resolution: {
          integrity: 'sha1-ChbBDewTLAqLCzb793Fo5VDvg/g=',
        },
      },
    },
  })
})

test('remove redundant linked package', () => {
  expect(pruneLockfile({
    importers: {
      ['.' as ProjectId]: {
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
  }, '.' as ProjectId, DEFAULT_OPTS)).toStrictEqual({
    importers: {
      '.': {
        specifiers: {},
      },
    },
    lockfileVersion: LOCKFILE_VERSION,
  })
})

test('keep all', () => {
  expect(pruneLockfile({
    importers: {
      ['.' as ProjectId]: {
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
      ['is-negative@1.0.0' as DepPath]: {
        dependencies: {
          'is-positive': '2.0.0',
        },
        resolution: {
          integrity: 'sha1-ChbBDewTLAqLCzb793Fo5VDvg/g=',
        },
      },
      ['is-positive@1.0.0' as DepPath]: {
        resolution: {
          integrity: 'sha1-ChbBDewTLAqLCzb793Fo5VDvg/g=',
        },
      },
      ['is-positive@2.0.0' as DepPath]: {
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
  }, '.' as ProjectId, DEFAULT_OPTS)).toStrictEqual({
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
      'is-negative@1.0.0': {
        dependencies: {
          'is-positive': '2.0.0',
        },
        resolution: {
          integrity: 'sha1-ChbBDewTLAqLCzb793Fo5VDvg/g=',
        },
      },
      'is-positive@1.0.0': {
        resolution: {
          integrity: 'sha1-ChbBDewTLAqLCzb793Fo5VDvg/g=',
        },
      },
      'is-positive@2.0.0': {
        resolution: {
          integrity: 'sha1-ChbBDewTLAqLCzb793Fo5VDvg/g=',
        },
      },
    },
  })
})

test('optional dependency should have optional = true', () => {
  expect(pruneLockfile({
    importers: {
      ['.' as ProjectId]: {
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
      ['foo-child@1.0.0' as DepPath]: {
        optional: true,
        resolution: {
          integrity: 'sha1-ChbBDewTLAqLCzb793Fo5VDvg/g=',
        },
      },
      ['foo@1.0.0' as DepPath]: {
        dependencies: {
          'foo-child': '1.0.0',
        },
        optional: true,
        resolution: {
          integrity: 'sha1-ChbBDewTLAqLCzb793Fo5VDvg/g=',
        },
      },
      ['is-positive@1.0.0' as DepPath]: {
        resolution: {
          integrity: 'sha1-ChbBDewTLAqLCzb793Fo5VDvg/g=',
        },
      },
      ['parent-of-foo@1.0.0' as DepPath]: {
        dependencies: {
          foo: '1.0.0',
        },
        resolution: {
          integrity: 'sha1-ChbBDewTLAqLCzb793Fo5VDvg/g=',
        },
      },
      ['pkg-with-good-optional@1.0.0' as DepPath]: {
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
  }, '.' as ProjectId, DEFAULT_OPTS)).toStrictEqual({
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
      'foo-child@1.0.0': {
        resolution: {
          integrity: 'sha1-ChbBDewTLAqLCzb793Fo5VDvg/g=',
        },
      },
      'foo@1.0.0': {
        dependencies: {
          'foo-child': '1.0.0',
        },
        resolution: {
          integrity: 'sha1-ChbBDewTLAqLCzb793Fo5VDvg/g=',
        },
      },
      'is-positive@1.0.0': {
        optional: true,
        resolution: {
          integrity: 'sha1-ChbBDewTLAqLCzb793Fo5VDvg/g=',
        },
      },
      'parent-of-foo@1.0.0': {
        dependencies: {
          foo: '1.0.0',
        },
        resolution: {
          integrity: 'sha1-ChbBDewTLAqLCzb793Fo5VDvg/g=',
        },
      },
      'pkg-with-good-optional@1.0.0': {
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
})

test('optional dependency should not have optional = true if used not only as optional', () => {
  expect(pruneLockfile({
    importers: {
      ['.' as ProjectId]: {
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
      ['is-positive@1.0.0' as DepPath]: {
        resolution: {
          integrity: 'sha1-ChbBDewTLAqLCzb793Fo5VDvg/g=',
        },
      },
      ['pkg-with-good-optional@1.0.0' as DepPath]: {
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
  }, '.' as ProjectId, DEFAULT_OPTS)).toStrictEqual({
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
      'is-positive@1.0.0': {
        resolution: {
          integrity: 'sha1-ChbBDewTLAqLCzb793Fo5VDvg/g=',
        },
      },
      'pkg-with-good-optional@1.0.0': {
        optionalDependencies: {
          'is-positive': '1.0.0',
        },
        resolution: {
          integrity: 'sha1-ChbBDewTLAqLCzb793Fo5VDvg/g=',
        },
      },
    },
  })
})

test('subdependency is both optional and dev', () => {
  expect(pruneLockfile(yaml`
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
      parent@1.0.0:
        optionalDependencies:
          subdep: 1.0.0
          subdep2: 1.0.0
        resolution:
          integrity: sha1-Sb1jMdfQLQwJvJEKEHW6gWW1bfk=
      prod-parent@1.0.0:
        dependencies:
          subdep2: 1.0.0
        resolution:
          integrity: sha1-Sb1jMdfQLQwJvJEKEHW6gWW1bfk=
      subdep@1.0.0:
        resolution:
          integrity: sha1-Sb1jMdfQLQwJvJEKEHW6gWW1bfk=
      subdep2@1.0.0:
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
  }, '.' as ProjectId, DEFAULT_OPTS)).toStrictEqual(yaml`
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
      parent@1.0.0:
        optionalDependencies:
          subdep: 1.0.0
          subdep2: 1.0.0
        resolution:
          integrity: sha1-Sb1jMdfQLQwJvJEKEHW6gWW1bfk=
      prod-parent@1.0.0:
        dependencies:
          subdep2: 1.0.0
        resolution:
          integrity: sha1-Sb1jMdfQLQwJvJEKEHW6gWW1bfk=
      subdep@1.0.0:
        optional: true
        resolution:
          integrity: sha1-Sb1jMdfQLQwJvJEKEHW6gWW1bfk=
      subdep2@1.0.0:
        resolution:
          integrity: sha1-Sb1jMdfQLQwJvJEKEHW6gWW1bfk=
  `)
})

test('optional = true is removed if dependency is used both as optional and prod dependency', () => {
  expect(pruneLockfile(yaml`
    importers:
      .:
        dependencies:
          foo: inflight@1.0.6
        optionalDependencies:
          inflight: 1.0.6
        specifiers:
          foo: 'npm:inflight@^1.0.6'
          inflight: ^1.0.6
    lockfileVersion: 5
    packages:
      inflight@1.0.6:
        optional: true
        dependencies:
          once: 1.4.0
          wrappy: 1.0.2
        resolution:
          integrity: sha1-Sb1jMdfQLQwJvJEKEHW6gWW1bfk=
      once@1.4.0:
        optional: true
        dependencies:
          wrappy: 1.0.2
        resolution:
          integrity: sha1-WDsap3WWHUsROsF9nFC6753Xa9E=
      wrappy@1.0.2:
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
  }, '.' as ProjectId, DEFAULT_OPTS)).toStrictEqual(yaml`
    importers:
      .:
        dependencies:
          foo: inflight@1.0.6
        optionalDependencies:
          inflight: 1.0.6
        specifiers:
          foo: 'npm:inflight@^1.0.6'
          inflight: ^1.0.6
    lockfileVersion: 5
    packages:
      inflight@1.0.6:
        dependencies:
          once: 1.4.0
          wrappy: 1.0.2
        resolution:
          integrity: sha1-Sb1jMdfQLQwJvJEKEHW6gWW1bfk=
      once@1.4.0:
        dependencies:
          wrappy: 1.0.2
        resolution:
          integrity: sha1-WDsap3WWHUsROsF9nFC6753Xa9E=
      wrappy@1.0.2:
        resolution:
          integrity: sha1-tSQ9jz7BqjXxNkYFvA0QNuMKtp8=
  `)
})

test('remove dependencies that are not in the package', () => {
  expect(pruneLockfile({
    importers: {
      ['.' as ProjectId]: {
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
      ['fsevents@1.0.0' as DepPath]: {
        resolution: {
          integrity: 'sha1-ChbBDewTLAqLCzb793Fo5VDvg/g=',
        },
      },
      ['is-negative@1.0.0' as DepPath]: {
        resolution: {
          integrity: 'sha1-ChbBDewTLAqLCzb793Fo5VDvg/g=',
        },
      },
      ['is-positive@1.0.0' as DepPath]: {
        resolution: {
          integrity: 'sha1-ChbBDewTLAqLCzb793Fo5VDvg/g=',
        },
      },
    },
  }, {
    name: 'foo',
    version: '1.0.0',
  }, '.' as ProjectId, DEFAULT_OPTS)).toStrictEqual({
    importers: {
      '.': {
        specifiers: {},
      },
    },
    lockfileVersion: LOCKFILE_VERSION,
  })
})

test(`ignore dependencies that are in package.json but are not in ${WANTED_LOCKFILE}`, () => {
  expect(pruneLockfile({
    importers: {
      ['.' as ProjectId]: {
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
      ['is-positive@1.0.0' as DepPath]: {
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
  }, '.' as ProjectId, DEFAULT_OPTS)).toStrictEqual({
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
      'is-positive@1.0.0': {
        resolution: {
          integrity: 'sha1-ChbBDewTLAqLCzb793Fo5VDvg/g=',
        },
      },
    },
  })
})

// this test may be redundant
test('keep lockfileMinorVersion, if present', () => {
  expect(pruneLockfile({
    importers: {
      ['.' as ProjectId]: {
        dependencies: {
          'is-positive': '1.0.0',
        },
        specifiers: {
          'is-positive': '^1.0.0',
        },
      },
    },
    lockfileVersion: '5.2',
    packages: {
      ['is-positive@1.0.0' as DepPath]: {
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
  }, '.' as ProjectId, DEFAULT_OPTS)).toStrictEqual({
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
    lockfileVersion: '5.2',
    packages: {
      'is-positive@1.0.0': {
        resolution: {
          integrity: 'sha1-ChbBDewTLAqLCzb793Fo5VDvg/g=',
        },
      },
    },
  })
})

test('keep linked package even if it is not in package.json', () => {
  expect(pruneLockfile({
    importers: {
      ['.' as ProjectId]: {
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
      ['is-negative@1.0.0' as DepPath]: {
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
  }, '.' as ProjectId, DEFAULT_OPTS)).toStrictEqual({
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
      'is-negative@1.0.0': {
        resolution: {
          integrity: 'sha1-ChbBDewTLAqLCzb793Fo5VDvg/g=',
        },
      },
    },
  })
})

test("prune: don't remove package used by another importer", () => {
  expect(pruneLockfile({
    importers: {
      ['packages/package-1' as ProjectId]: {
        dependencies: {
          'is-positive': '1.0.0',
        },
        specifiers: {
          'is-positive': '^1.0.0',
        },
      },
      ['packages/package-2' as ProjectId]: {
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
      ['is-negative@1.0.0' as DepPath]: {
        resolution: {
          integrity: 'sha1-ChbBDewTLAqLCzb793Fo5VDvg/g=',
        },
      },
      ['is-positive@1.0.0' as DepPath]: {
        resolution: {
          integrity: 'sha1-ChbBDewTLAqLCzb793Fo5VDvg/g=',
        },
      },
      ['is-positive@2.0.0' as DepPath]: {
        resolution: {
          integrity: 'sha1-ChbBDewTLAqLCzb793Fo5VDvg/g=',
        },
      },
    },
  }, {
    name: 'project-2',
    version: '1.0.0',

    dependencies: { 'is-negative': '^1.0.0' },
  }, 'packages/package-2' as ProjectId, DEFAULT_OPTS)).toStrictEqual({
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
      'is-negative@1.0.0': {
        resolution: {
          integrity: 'sha1-ChbBDewTLAqLCzb793Fo5VDvg/g=',
        },
      },
      'is-positive@1.0.0': {
        resolution: {
          integrity: 'sha1-ChbBDewTLAqLCzb793Fo5VDvg/g=',
        },
      },
    },
  })
})

test('pruneSharedLockfile: remove one redundant package', () => {
  expect(pruneSharedLockfile({
    importers: {
      ['packages/package-1' as ProjectId]: {
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
      ['is-positive@1.0.0' as DepPath]: {
        resolution: {
          integrity: 'sha1-ChbBDewTLAqLCzb793Fo5VDvg/g=',
        },
      },
      ['is-positive@2.0.0' as DepPath]: {
        resolution: {
          integrity: 'sha1-ChbBDewTLAqLCzb793Fo5VDvg/g=',
        },
      },
    },
  }, DEFAULT_OPTS)).toStrictEqual({
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
      'is-positive@1.0.0': {
        resolution: {
          integrity: 'sha1-ChbBDewTLAqLCzb793Fo5VDvg/g=',
        },
      },
    },
  })
})
