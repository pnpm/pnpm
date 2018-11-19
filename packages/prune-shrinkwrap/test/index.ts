///<reference path="../typings/local.d.ts"/>
import {
  prune,
  pruneSharedShrinkwrap,
} from '@pnpm/prune-shrinkwrap'
import test = require('tape')
import yaml = require('yaml-tag')

const DEFAULT_OPTS = {
  defaultRegistry: 'https://registry.npmjs.org',
  warn (msg: string) {
    // ignore
  },
}

test('remove one redundant package', t => {
  t.deepEqual(prune({
    importers: {
      '.': {
        dependencies: {
          'is-positive': '1.0.0'
        },
        specifiers: {
          'is-positive': '^1.0.0'
        },
      },
    },
    packages: {
      '/is-positive/1.0.0': {
        dev: false,
        resolution: {
          integrity: 'sha1-ChbBDewTLAqLCzb793Fo5VDvg/g='
        }
      },
      '/is-positive/2.0.0': {
        dev: false,
        resolution: {
          integrity: 'sha1-ChbBDewTLAqLCzb793Fo5VDvg/g='
        },
      },
    },
    registry: 'https://registry.npmjs.org',
    shrinkwrapVersion: 3,
  }, {
    name: 'foo',
    version: '1.0.0',

    dependencies: {
      'is-positive': '^1.0.0'
    }
  }, '.', DEFAULT_OPTS), {
    importers: {
      '.': {
        dependencies: {
          'is-positive': '1.0.0'
        },
        specifiers: {
          'is-positive': '^1.0.0'
        },
      },
    },
    packages: {
      '/is-positive/1.0.0': {
        dev: false,
        resolution: {
          integrity: 'sha1-ChbBDewTLAqLCzb793Fo5VDvg/g='
        },
      },
    },
    registry: 'https://registry.npmjs.org',
    shrinkwrapVersion: 3,
  })

  t.end()
})

test('remove redundant linked package', t => {
  t.deepEqual(prune({
    importers: {
      '.': {
        dependencies: {
          'is-positive': 'link:../is-positive'
        },
        specifiers: {
          'is-positive': '^1.0.0'
        },
      },
    },
    packages: {},
    registry: 'https://registry.npmjs.org',
    shrinkwrapVersion: 3,
  }, {
    name: 'foo',
    version: '1.0.0',

    dependencies: {}
  }, '.', DEFAULT_OPTS), {
    importers: {
      '.': {
        specifiers: {},
      },
    },
    registry: 'https://registry.npmjs.org',
    shrinkwrapVersion: 3,
  })

  t.end()
})

test('keep all', t => {
  t.deepEqual(prune({
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
    packages: {
      '/is-negative/1.0.0': {
        dependencies: {
          'is-positive': '2.0.0',
        },
        dev: false,
        resolution: {
          integrity: 'sha1-ChbBDewTLAqLCzb793Fo5VDvg/g='
        }
      },
      '/is-positive/1.0.0': {
        dev: false,
        resolution: {
          integrity: 'sha1-ChbBDewTLAqLCzb793Fo5VDvg/g='
        }
      },
      '/is-positive/2.0.0': {
        dev: false,
        resolution: {
          integrity: 'sha1-ChbBDewTLAqLCzb793Fo5VDvg/g='
        }
      },
    },
    registry: 'https://registry.npmjs.org',
    shrinkwrapVersion: 3,
  }, {
    name: 'foo',
    version: '1.0.0',

    dependencies: {
      'is-negative': '^1.0.0',
      'is-positive': '^1.0.0',
    }
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
    packages: {
      '/is-negative/1.0.0': {
        dependencies: {
          'is-positive': '2.0.0',
        },
        dev: false,
        resolution: {
          integrity: 'sha1-ChbBDewTLAqLCzb793Fo5VDvg/g='
        }
      },
      '/is-positive/1.0.0': {
        dev: false,
        resolution: {
          integrity: 'sha1-ChbBDewTLAqLCzb793Fo5VDvg/g='
        }
      },
      '/is-positive/2.0.0': {
        dev: false,
        resolution: {
          integrity: 'sha1-ChbBDewTLAqLCzb793Fo5VDvg/g='
        }
      },
    },
    registry: 'https://registry.npmjs.org',
    shrinkwrapVersion: 3,
  })

  t.end()
})

test('optional dependency should have optional = true', t => {
  t.deepEqual(prune({
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
          integrity: 'sha1-ChbBDewTLAqLCzb793Fo5VDvg/g='
        }
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
          integrity: 'sha1-ChbBDewTLAqLCzb793Fo5VDvg/g='
        }
      },
    },
    registry: 'https://registry.npmjs.org',
    shrinkwrapVersion: 3,
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
          integrity: 'sha1-ChbBDewTLAqLCzb793Fo5VDvg/g='
        }
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
          integrity: 'sha1-ChbBDewTLAqLCzb793Fo5VDvg/g='
        }
      },
    },
    registry: 'https://registry.npmjs.org',
    shrinkwrapVersion: 3,
  })

  t.end()
})

test('optional dependency should not have optional = true if used not only as optional', t => {
  t.deepEqual(prune({
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
    packages: {
      '/is-positive/1.0.0': {
        dev: false,
        resolution: {
          integrity: 'sha1-ChbBDewTLAqLCzb793Fo5VDvg/g='
        }
      },
      '/pkg-with-good-optional/1.0.0': {
        dev: false,
        optionalDependencies: {
          'is-positive': '1.0.0',
        },
        resolution: {
          integrity: 'sha1-ChbBDewTLAqLCzb793Fo5VDvg/g='
        }
      },
    },
    registry: 'https://registry.npmjs.org',
    shrinkwrapVersion: 3,
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
    packages: {
      '/is-positive/1.0.0': {
        dev: false,
        resolution: {
          integrity: 'sha1-ChbBDewTLAqLCzb793Fo5VDvg/g='
        }
      },
      '/pkg-with-good-optional/1.0.0': {
        dev: false,
        optionalDependencies: {
          'is-positive': '1.0.0',
        },
        resolution: {
          integrity: 'sha1-ChbBDewTLAqLCzb793Fo5VDvg/g='
        },
      },
    },
    registry: 'https://registry.npmjs.org',
    shrinkwrapVersion: 3,
  })

  t.end()
})

test('dev dependency should have dev = true', t => {
  t.deepEqual(prune({
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
    packages: {
      '/is-positive/1.0.0': {
        resolution: {
          integrity: 'sha1-ChbBDewTLAqLCzb793Fo5VDvg/g='
        }
      },
      '/pkg-with-good-optional/1.0.0': {
        dev: false,
        resolution: {
          integrity: 'sha1-ChbBDewTLAqLCzb793Fo5VDvg/g='
        },
      },
    },
    registry: 'https://registry.npmjs.org',
    shrinkwrapVersion: 3,
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
    packages: {
      '/is-positive/1.0.0': {
        dev: true,
        resolution: {
          integrity: 'sha1-ChbBDewTLAqLCzb793Fo5VDvg/g='
        }
      },
      '/pkg-with-good-optional/1.0.0': {
        dev: false,
        resolution: {
          integrity: 'sha1-ChbBDewTLAqLCzb793Fo5VDvg/g='
        },
      },
    },
    registry: 'https://registry.npmjs.org',
    shrinkwrapVersion: 3,
  })

  t.end()
})

test('dev dependency should not have dev = true if it is used not only as dev', t => {
  t.deepEqual(prune({
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
    packages: {
      '/is-positive/1.0.0': {
        resolution: {
          integrity: 'sha1-ChbBDewTLAqLCzb793Fo5VDvg/g='
        }
      },
      '/some-pkg/1.0.0': {
        dependencies: {
          'is-positive': '1.0.0',
        },
        dev: false,
        resolution: {
          integrity: 'sha1-ChbBDewTLAqLCzb793Fo5VDvg/g='
        },
      },
    },
    registry: 'https://registry.npmjs.org',
    shrinkwrapVersion: 3,
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
    packages: {
      '/is-positive/1.0.0': {
        resolution: {
          integrity: 'sha1-ChbBDewTLAqLCzb793Fo5VDvg/g='
        }
      },
      '/some-pkg/1.0.0': {
        dependencies: {
          'is-positive': '1.0.0',
        },
        dev: false,
        resolution: {
          integrity: 'sha1-ChbBDewTLAqLCzb793Fo5VDvg/g='
        },
      },
    },
    registry: 'https://registry.npmjs.org',
    shrinkwrapVersion: 3,
  })

  t.end()
})

test('the dev field should be updated to dev = false if it is not a dev dependency anymore', t => {
  t.deepEqual(prune({
    importers: {
      '.': {
        dependencies: {
          'a': '1.0.0',
        },
        specifiers: {
          'a': '^1.0.0',
        },
      },
    },
    packages: {
      '/a/1.0.0': {
        resolution: {
          integrity: 'sha1-ChbBDewTLAqLCzb793Fo5VDvg/g='
        },
      },
    },
    registry: 'https://registry.npmjs.org',
    shrinkwrapVersion: 3,
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
          'a': '1.0.0',
        },
        specifiers: {
          'a': '^1.0.0',
        },
      },
    },
    packages: {
      '/a/1.0.0': {
        dev: false,
        resolution: {
          integrity: 'sha1-ChbBDewTLAqLCzb793Fo5VDvg/g='
        },
      },
    },
    registry: 'https://registry.npmjs.org',
    shrinkwrapVersion: 3,
  })

  t.end()
})

test('subdependency is both optional and dev', t => {
  t.deepEqual(prune(yaml`
    importers:
      .:
        dependencies:
          prod-parent: 1.0.0
        devDependencies:
          parent: 1.0.0
        specifiers:
          parent: ^1.0.0
          prod-parent: ^1.0.0
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
    registry: 'http://localhost:4873/'
    shrinkwrapVersion: 3
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
    registry: 'http://localhost:4873/'
    shrinkwrapVersion: 3
  `)

  t.end()
})

test('dev = true is removed if dependency is used both as dev and prod dependency', t => {
  t.deepEqual(prune(yaml`
    importers:
      .:
        dependencies:
          foo: /inflight/1.0.6
        devDependencies:
          inflight: 1.0.6
        specifiers:
          foo: 'npm:inflight@^1.0.6'
          inflight: ^1.0.6
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
    registry: 'http://localhost:4873/'
    shrinkwrapVersion: 3
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
    registry: 'http://localhost:4873/'
    shrinkwrapVersion: 3
  `)

  t.end()
})

test('optional = true is removed if dependency is used both as optional and prod dependency', t => {
  t.deepEqual(prune(yaml`
    importers:
      .:
        dependencies:
          foo: /inflight/1.0.6
        optionalDependencies:
          inflight: 1.0.6
        specifiers:
          foo: 'npm:inflight@^1.0.6'
          inflight: ^1.0.6
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
    registry: 'http://localhost:4873/'
    shrinkwrapVersion: 3
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
    registry: 'http://localhost:4873/'
    shrinkwrapVersion: 3
  `)

  t.end()
})

test('remove dependencies that are not in the package', t => {
  t.deepEqual(prune({
    importers: {
      '.': {
        dependencies: {
          'is-positive': '1.0.0'
        },
        devDependencies: {
          'is-negative': '1.0.0'
        },
        optionalDependencies: {
          'fsevents': '1.0.0'
        },
        specifiers: {
          'fsevents': '^1.0.0',
          'is-negative': '^1.0.0',
          'is-positive': '^1.0.0',
        },
      },
    },
    packages: {
      '/fsevents/1.0.0': {
        resolution: {
          integrity: 'sha1-ChbBDewTLAqLCzb793Fo5VDvg/g='
        },
      },
      '/is-negative/1.0.0': {
        resolution: {
          integrity: 'sha1-ChbBDewTLAqLCzb793Fo5VDvg/g='
        }
      },
      '/is-positive/1.0.0': {
        resolution: {
          integrity: 'sha1-ChbBDewTLAqLCzb793Fo5VDvg/g='
        }
      },
    },
    registry: 'https://registry.npmjs.org',
    shrinkwrapVersion: 3,
  }, {
    name: 'foo',
    version: '1.0.0',
  }, '.', DEFAULT_OPTS), {
    importers: {
      '.': {
        specifiers: {},
      },
    },
    registry: 'https://registry.npmjs.org',
    shrinkwrapVersion: 3,
  })

  t.end()
})

test('ignore dependencies that are in package.json but are not in shrinkwrap.yaml', t => {
  t.deepEqual(prune({
    importers: {
      '.': {
        dependencies: {
          'is-positive': '1.0.0'
        },
        specifiers: {
          'is-positive': '^1.0.0'
        },
      },
    },
    packages: {
      '/is-positive/1.0.0': {
        dev: false,
        resolution: {
          integrity: 'sha1-ChbBDewTLAqLCzb793Fo5VDvg/g='
        },
      },
    },
    registry: 'https://registry.npmjs.org',
    shrinkwrapVersion: 3,
  }, {
    name: 'foo',
    version: '1.0.0',

    dependencies: {
      'is-negative': '^1.0.0',
      'is-positive': '^1.0.0',
    }
  }, '.', DEFAULT_OPTS), {
    importers: {
      '.': {
        dependencies: {
          'is-positive': '1.0.0'
        },
        specifiers: {
          'is-positive': '^1.0.0'
        },
      },
    },
    packages: {
      '/is-positive/1.0.0': {
        dev: false,
        resolution: {
          integrity: 'sha1-ChbBDewTLAqLCzb793Fo5VDvg/g='
        },
      },
    },
    registry: 'https://registry.npmjs.org',
    shrinkwrapVersion: 3,
  })

  t.end()
})

// this test may be redundant
test('keep shrinkwrapMinorVersion, if present', t => {
  t.deepEqual(prune({
    importers: {
      '.': {
        dependencies: {
          'is-positive': '1.0.0'
        },
        specifiers: {
          'is-positive': '^1.0.0'
        },
      },
    },
    packages: {
      '/is-positive/1.0.0': {
        dev: false,
        resolution: {
          integrity: 'sha1-ChbBDewTLAqLCzb793Fo5VDvg/g='
        },
      },
    },
    registry: 'https://registry.npmjs.org',
    shrinkwrapVersion: 3.2,
  }, {
    name: 'foo',
    version: '1.0.0',

    dependencies: {
      'is-positive': '^1.0.0',
    }
  }, '.', DEFAULT_OPTS), {
    importers: {
      '.': {
        dependencies: {
          'is-positive': '1.0.0'
        },
        specifiers: {
          'is-positive': '^1.0.0'
        },
      },
    },
    packages: {
      '/is-positive/1.0.0': {
        dev: false,
        resolution: {
          integrity: 'sha1-ChbBDewTLAqLCzb793Fo5VDvg/g='
        },
      },
    },
    registry: 'https://registry.npmjs.org',
    shrinkwrapVersion: 3.2,
  })

  t.end()
})

test('keep linked package even if it is not in package.json', t => {
  t.deepEqual(prune({
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
    packages: {
      '/is-negative/1.0.0': {
        dev: false,
        resolution: {
          integrity: 'sha1-ChbBDewTLAqLCzb793Fo5VDvg/g=',
        },
      },
    },
    registry: 'https://registry.npmjs.org',
    shrinkwrapVersion: 3,
  }, {
    name: 'foo',
    version: '1.0.0',

    dependencies: {
      'is-negative': '^1.0.0'
    }
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
    packages: {
      '/is-negative/1.0.0': {
        dev: false,
        resolution: {
          integrity: 'sha1-ChbBDewTLAqLCzb793Fo5VDvg/g=',
        },
      },
    },
    registry: 'https://registry.npmjs.org',
    shrinkwrapVersion: 3,
  })

  t.end()
})

test("prune: don't remove package used by another importer", t => {
  t.deepEqual(prune({
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
    packages: {
      '/is-negative/1.0.0': {
        dev: false,
        resolution: {
          integrity: 'sha1-ChbBDewTLAqLCzb793Fo5VDvg/g='
        },
      },
      '/is-positive/1.0.0': {
        dev: false,
        resolution: {
          integrity: 'sha1-ChbBDewTLAqLCzb793Fo5VDvg/g='
        }
      },
      '/is-positive/2.0.0': {
        dev: false,
        resolution: {
          integrity: 'sha1-ChbBDewTLAqLCzb793Fo5VDvg/g='
        }
      },
    },
    registry: 'https://registry.npmjs.org',
    shrinkwrapVersion: 3,
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
    registry: 'https://registry.npmjs.org',
    shrinkwrapVersion: 3,
  })

  t.end()
})

test('pruneSharedShrinkwrap: remove one redundant package', t => {
  t.deepEqual(pruneSharedShrinkwrap({
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
    packages: {
      '/is-positive/1.0.0': {
        dev: false,
        resolution: {
          integrity: 'sha1-ChbBDewTLAqLCzb793Fo5VDvg/g='
        }
      },
      '/is-positive/2.0.0': {
        dev: false,
        resolution: {
          integrity: 'sha1-ChbBDewTLAqLCzb793Fo5VDvg/g='
        },
      },
    },
    registry: 'https://registry.npmjs.org',
    shrinkwrapVersion: 3,
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
    packages: {
      '/is-positive/1.0.0': {
        dev: false,
        resolution: {
          integrity: 'sha1-ChbBDewTLAqLCzb793Fo5VDvg/g='
        },
      },
    },
    registry: 'https://registry.npmjs.org',
    shrinkwrapVersion: 3,
  })

  t.end()
})
