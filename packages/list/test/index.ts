///<reference path="../../../typings/index.d.ts"/>
import list, { forPackages as listForPackages } from '@pnpm/list'
import renderTree from '@pnpm/list/lib/renderTree'
import chalk from 'chalk'
import { stripIndent } from 'common-tags'
import path = require('path')
import test = require('tape')

const highlighted = chalk.yellow.bgBlack

const fixture = path.join(__dirname, 'fixture')
const fixtureWithNoPkgNameAndNoVersion = path.join(__dirname, 'fixture-with-no-pkg-name-and-no-version')
const fixtureWithNoPkgVersion = path.join(__dirname, 'fixture-with-no-pkg-version')
const fixtureWithExternalLockfile = path.join(__dirname, 'fixture-with-external-shrinkwrap', 'pkg')
const emptyFixture = path.join(__dirname, 'empty')
const fixtureWithAliasedDep = path.join(__dirname, 'with-aliased-dep')

test('list all deps of a package that has an external lockfile', async (t) => {
  t.equal(await list(fixtureWithExternalLockfile, {
    lockfileDirectory: path.join(fixtureWithExternalLockfile, '..'),
  }), stripIndent`
    pkg@1.0.0 ${fixtureWithExternalLockfile}
    └── is-positive@1.0.0
  `)

  t.end()
})

test('list with default parameters', async t => {
  t.equal(await list(fixture), stripIndent`
    fixture@1.0.0 ${fixture}
    ├── is-negative@2.1.0
    ├── is-positive@3.1.0
    └── write-json-file@2.2.0
  `)

  t.end()
})

test('list with default parameters in pkg that has no name and version', async t => {
  t.equal(await list(fixtureWithNoPkgNameAndNoVersion), stripIndent`
    ${fixtureWithNoPkgNameAndNoVersion}
    ├── is-negative@2.1.0
    ├── is-positive@3.1.0
    └── write-json-file@2.2.0
  `)

  t.end()
})

test('list with default parameters in pkg that has no version', async t => {
  t.equal(await list(fixtureWithNoPkgVersion), stripIndent`
    fixture ${fixtureWithNoPkgVersion}
    ├── is-negative@2.1.0
    ├── is-positive@3.1.0
    └── write-json-file@2.2.0
  `)

  t.end()
})

test('list dev only', async t => {
  t.equal(await list(fixture, { only: 'dev' }), stripIndent`
    fixture@1.0.0 ${fixture}
    └── is-positive@3.1.0
  `)

  t.end()
})

test('list prod only', async t => {
  t.equal(await list(fixture, { only: 'prod' }), stripIndent`
    fixture@1.0.0 ${fixture}
    └── write-json-file@2.2.0
  `)

  t.end()
})

test('list prod only with depth 2', async t => {
  t.equal(await list(fixture, { only: 'prod', depth: 2 }), stripIndent`
    fixture@1.0.0 ${fixture}
    └─┬ write-json-file@2.2.0
      ├── detect-indent@5.0.0
      ├── graceful-fs@4.1.11
      ├─┬ make-dir@1.0.0
      │ └── pify@2.3.0
      ├── pify@2.3.0
      ├─┬ sort-keys@1.1.2
      │ └── is-plain-obj@1.1.0
      └─┬ write-file-atomic@2.1.0
        ├── graceful-fs@4.1.11
        ├── imurmurhash@0.1.4
        └── slide@1.1.6
  `)

  t.end()
})

test('list with depth 1', async t => {
  t.equal(await list(fixture, { depth: 1 }), stripIndent`
    fixture@1.0.0 ${fixture}
    ├── is-negative@2.1.0
    ├── is-positive@3.1.0
    └─┬ write-json-file@2.2.0
      ├── detect-indent@5.0.0
      ├── graceful-fs@4.1.11
      ├── make-dir@1.0.0
      ├── pify@2.3.0
      ├── sort-keys@1.1.2
      └── write-file-atomic@2.1.0
  `)

  t.end()
})

test('list with depth -1', async t => {
  t.equal(await list(fixture, { depth: -1 }), `fixture@1.0.0 ${fixture}`)

  t.end()
})

test('list with depth 1 and selected packages', async t => {
  t.equal(await listForPackages(['make-dir', 'pify@2', 'sort-keys@2', 'is-negative'], fixture, { depth: 1 }), stripIndent`
    fixture@1.0.0 ${fixture}
    ├── ${highlighted('is-negative@2.1.0')}
    └─┬ write-json-file@2.2.0
      ├── ${highlighted('make-dir@1.0.0')}
      └── ${highlighted('pify@2.3.0')}
  `)

  t.end()
})

test('list in long format', async t => {
  t.equal(await list(fixture, { long: true }), stripIndent`
    fixture@1.0.0 ${fixture}
    ├── is-negative@2.1.0
    │   Check if something is a negative number
    │   git+https://github.com/kevva/is-negative.git
    │   https://github.com/kevva/is-negative#readme
    ├── is-positive@3.1.0
    │   Check if something is a positive number
    │   git+https://github.com/kevva/is-positive.git
    │   https://github.com/kevva/is-positive#readme
    └── write-json-file@2.2.0
        Stringify and write JSON to a file atomically
        git+https://github.com/sindresorhus/write-json-file.git
        https://github.com/sindresorhus/write-json-file#readme
  `)

  t.end()
})

test('parseable list with depth 1', async t => {
  t.equal(await list(fixture, { reportAs: 'parseable', depth: 1 }), stripIndent`
    ${fixture}
    ${path.join(fixture, 'node_modules/.registry.npmjs.org/detect-indent/5.0.0')}
    ${path.join(fixture, 'node_modules/.registry.npmjs.org/graceful-fs/4.1.11')}
    ${path.join(fixture, 'node_modules/.registry.npmjs.org/is-negative/2.1.0')}
    ${path.join(fixture, 'node_modules/.registry.npmjs.org/is-positive/3.1.0')}
    ${path.join(fixture, 'node_modules/.registry.npmjs.org/make-dir/1.0.0')}
    ${path.join(fixture, 'node_modules/.registry.npmjs.org/pify/2.3.0')}
    ${path.join(fixture, 'node_modules/.registry.npmjs.org/sort-keys/1.1.2')}
    ${path.join(fixture, 'node_modules/.registry.npmjs.org/write-file-atomic/2.1.0')}
    ${path.join(fixture, 'node_modules/.registry.npmjs.org/write-json-file/2.2.0')}
    `)

  t.end()
})

test('JSON list with depth 1', async t => {
  t.equal(await list(fixture, { reportAs: 'json', depth: 1 }), JSON.stringify({
    name: 'fixture',
    version: '1.0.0',

    dependencies: {
      'is-negative': {
        from: 'is-negative',
        version: '2.1.0',

        resolved: 'https://registry.npmjs.org/is-negative/-/is-negative-2.1.0.tgz',
      },
      'is-positive': {
        from: 'is-positive',
        version: '3.1.0',

        resolved: 'https://registry.npmjs.org/is-positive/-/is-positive-3.1.0.tgz',
      },
      'write-json-file': {
        from: 'write-json-file',
        version: '2.2.0',

        resolved: 'https://registry.npmjs.org/write-json-file/-/write-json-file-2.2.0.tgz',

        dependencies: {
          'detect-indent': {
            from: 'detect-indent',
            version: '5.0.0',

            resolved: 'https://registry.npmjs.org/detect-indent/-/detect-indent-5.0.0.tgz',
          },
          'graceful-fs': {
            from: 'graceful-fs',
            version: '4.1.11',

            resolved: 'https://registry.npmjs.org/graceful-fs/-/graceful-fs-4.1.11.tgz',
          },
          'make-dir': {
            from: 'make-dir',
            version: '1.0.0',

            resolved: 'https://registry.npmjs.org/make-dir/-/make-dir-1.0.0.tgz',
          },
          'pify': {
            from: 'pify',
            version: '2.3.0',

            resolved: 'https://registry.npmjs.org/pify/-/pify-2.3.0.tgz',
          },
          'sort-keys': {
            from: 'sort-keys',
            version: '1.1.2',

            resolved: 'https://registry.npmjs.org/sort-keys/-/sort-keys-1.1.2.tgz',
          },
          'write-file-atomic': {
            from: 'write-file-atomic',
            version: '2.1.0',

            resolved: 'https://registry.npmjs.org/write-file-atomic/-/write-file-atomic-2.1.0.tgz',
          }
        }
      }
    },
  }, null, 2))
  t.end()
})

test('JSON list with aliased dep', async t => {
  t.equal(await list(fixtureWithAliasedDep, { reportAs: 'json' }), JSON.stringify({
    name: 'with-aliased-dep',
    version: '1.0.0',

    dependencies: {
      'positive': {
        from: 'is-positive',
        version: '1.0.0',

        resolved: 'https://registry.npmjs.org/is-positive/-/is-positive-1.0.0.tgz',
      },
    },
  }, null, 2))
  t.equal(await list(fixtureWithAliasedDep, { long: true, reportAs: 'json' }), JSON.stringify({
    name: 'with-aliased-dep',
    version: '1.0.0',

    dependencies: {
      'positive': {
        from: 'is-positive',
        version: '1.0.0',

        resolved: 'https://registry.npmjs.org/is-positive/-/is-positive-1.0.0.tgz',

        description: 'Test if a number is positive',

        homepage: 'https://github.com/kevva/is-positive#readme',

        repository: 'git+https://github.com/kevva/is-positive.git',
      },
    },
  }, null, 2), 'with long info')
  t.end()
})

test('parseable list with depth 1 and dev only', async t => {
  t.equal(await list(fixture, { reportAs: 'parseable', depth: 1, only: 'dev' }), stripIndent`
    ${fixture}
    ${path.join(fixture, 'node_modules/.registry.npmjs.org/is-positive/3.1.0')}
  `)

  t.end()
})

test('long parseable list with depth 1', async t => {
  t.equal(await list(fixture, { reportAs: 'parseable', depth: 1, long: true }), stripIndent`
    ${fixture}:fixture@1.0.0
    ${path.join(fixture, 'node_modules/.registry.npmjs.org/detect-indent/5.0.0')}:detect-indent@5.0.0
    ${path.join(fixture, 'node_modules/.registry.npmjs.org/graceful-fs/4.1.11')}:graceful-fs@4.1.11
    ${path.join(fixture, 'node_modules/.registry.npmjs.org/is-negative/2.1.0')}:is-negative@2.1.0
    ${path.join(fixture, 'node_modules/.registry.npmjs.org/is-positive/3.1.0')}:is-positive@3.1.0
    ${path.join(fixture, 'node_modules/.registry.npmjs.org/make-dir/1.0.0')}:make-dir@1.0.0
    ${path.join(fixture, 'node_modules/.registry.npmjs.org/pify/2.3.0')}:pify@2.3.0
    ${path.join(fixture, 'node_modules/.registry.npmjs.org/sort-keys/1.1.2')}:sort-keys@1.1.2
    ${path.join(fixture, 'node_modules/.registry.npmjs.org/write-file-atomic/2.1.0')}:write-file-atomic@2.1.0
    ${path.join(fixture, 'node_modules/.registry.npmjs.org/write-json-file/2.2.0')}:write-json-file@2.2.0
  `)

  t.end()
})

test('long parseable list with depth 1 when package has no version', async t => {
  t.equal(await list(fixtureWithNoPkgVersion, { reportAs: 'parseable', depth: 1, long: true }), stripIndent`
    ${fixtureWithNoPkgVersion}:fixture
    ${path.join(fixtureWithNoPkgVersion, 'node_modules/.registry.npmjs.org/detect-indent/5.0.0')}:detect-indent@5.0.0
    ${path.join(fixtureWithNoPkgVersion, 'node_modules/.registry.npmjs.org/graceful-fs/4.1.11')}:graceful-fs@4.1.11
    ${path.join(fixtureWithNoPkgVersion, 'node_modules/.registry.npmjs.org/is-negative/2.1.0')}:is-negative@2.1.0
    ${path.join(fixtureWithNoPkgVersion, 'node_modules/.registry.npmjs.org/is-positive/3.1.0')}:is-positive@3.1.0
    ${path.join(fixtureWithNoPkgVersion, 'node_modules/.registry.npmjs.org/make-dir/1.0.0')}:make-dir@1.0.0
    ${path.join(fixtureWithNoPkgVersion, 'node_modules/.registry.npmjs.org/pify/2.3.0')}:pify@2.3.0
    ${path.join(fixtureWithNoPkgVersion, 'node_modules/.registry.npmjs.org/sort-keys/1.1.2')}:sort-keys@1.1.2
    ${path.join(fixtureWithNoPkgVersion, 'node_modules/.registry.npmjs.org/write-file-atomic/2.1.0')}:write-file-atomic@2.1.0
    ${path.join(fixtureWithNoPkgVersion, 'node_modules/.registry.npmjs.org/write-json-file/2.2.0')}:write-json-file@2.2.0
  `)

  t.end()
})

test('long parseable list with depth 1 when package has no name and no version', async t => {
  t.equal(await list(fixtureWithNoPkgNameAndNoVersion, { reportAs: 'parseable', depth: 1, long: true }), stripIndent`
    ${fixtureWithNoPkgNameAndNoVersion}
    ${path.join(fixtureWithNoPkgNameAndNoVersion, 'node_modules/.registry.npmjs.org/detect-indent/5.0.0')}:detect-indent@5.0.0
    ${path.join(fixtureWithNoPkgNameAndNoVersion, 'node_modules/.registry.npmjs.org/graceful-fs/4.1.11')}:graceful-fs@4.1.11
    ${path.join(fixtureWithNoPkgNameAndNoVersion, 'node_modules/.registry.npmjs.org/is-negative/2.1.0')}:is-negative@2.1.0
    ${path.join(fixtureWithNoPkgNameAndNoVersion, 'node_modules/.registry.npmjs.org/is-positive/3.1.0')}:is-positive@3.1.0
    ${path.join(fixtureWithNoPkgNameAndNoVersion, 'node_modules/.registry.npmjs.org/make-dir/1.0.0')}:make-dir@1.0.0
    ${path.join(fixtureWithNoPkgNameAndNoVersion, 'node_modules/.registry.npmjs.org/pify/2.3.0')}:pify@2.3.0
    ${path.join(fixtureWithNoPkgNameAndNoVersion, 'node_modules/.registry.npmjs.org/sort-keys/1.1.2')}:sort-keys@1.1.2
    ${path.join(fixtureWithNoPkgNameAndNoVersion, 'node_modules/.registry.npmjs.org/write-file-atomic/2.1.0')}:write-file-atomic@2.1.0
    ${path.join(fixtureWithNoPkgNameAndNoVersion, 'node_modules/.registry.npmjs.org/write-json-file/2.2.0')}:write-json-file@2.2.0
  `)

  t.end()
})

test('print empty', async t => {
  t.equal(await list(emptyFixture), `empty@1.0.0 ${emptyFixture}`)
  t.end()
})

test("don't print empty", async t => {
  t.equal(await list(emptyFixture, { alwaysPrintRootPackage: false }), '')
  t.end()
})

test('unsaved dependencies are marked', async (t) => {
  t.equal(await renderTree(
    {
      name: 'fixture',
      path: fixture,
      version: '1.0.0',
    },
    [
      {
        pkg: {
          alias: 'foo',
          name: 'foo',
          path: '',
          version: '1.0.0',
        },
        saved: false,
      },
    ],
    {
      alwaysPrintRootPackage: false,
      long: false,
    },
  ), stripIndent`
    fixture@1.0.0 ${fixture}
    └── foo@1.0.0 ${chalk.whiteBright.bgBlack('not saved')}
  `)
  t.end()
})
