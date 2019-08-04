///<reference path="../../../typings/index.d.ts"/>
import list, { forPackages as listForPackages } from '@pnpm/list'
import renderTree from '@pnpm/list/lib/renderTree'
import chalk from 'chalk'
import { stripIndent } from 'common-tags'
import path = require('path')
import test = require('tape')

const DEV_DEP_ONLY_CLR = chalk.yellow
const PROD_DEP_CLR = (s: string) => s // just use the default color
const OPTIONAL_DEP_CLR = chalk.blue
const NOT_SAVED_DEP_CLR = chalk.red
const VERSION_CLR = chalk.gray

const LEGEND = `Legend: ${PROD_DEP_CLR('production dependency')}, ${OPTIONAL_DEP_CLR('optional only')}, ${DEV_DEP_ONLY_CLR('dev only')}`
const DEPENDENCIES = chalk.cyanBright('dependencies:')
const DEV_DEPENDENCIES = chalk.cyanBright('devDependencies:')
const OPTIONAL_DEPENDENCIES = chalk.cyanBright('optionalDependencies:')
const UNSAVED_DEPENDENCIES = chalk.cyanBright('not saved (you should add these dependencies to package.json if you need them):')

const highlighted = chalk.bold.bgBlack

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
    ${LEGEND}

    pkg@1.0.0 ${fixtureWithExternalLockfile}

    ${DEPENDENCIES}
    is-positive ${VERSION_CLR('1.0.0')}
  `)

  t.end()
})

test('list with default parameters', async t => {
  t.equal(await list(fixture), stripIndent`
    ${LEGEND}

    fixture@1.0.0 ${fixture}

    ${DEPENDENCIES}
    write-json-file ${VERSION_CLR('2.2.0')}

    ${DEV_DEPENDENCIES}
    ${DEV_DEP_ONLY_CLR('is-positive')} ${VERSION_CLR('3.1.0')}

    ${OPTIONAL_DEPENDENCIES}
    ${OPTIONAL_DEP_CLR('is-negative')} ${VERSION_CLR('2.1.0')}
  `)

  t.end()
})

test('list with default parameters in pkg that has no name and version', async t => {
  t.equal(await list(fixtureWithNoPkgNameAndNoVersion), stripIndent`
    ${LEGEND}

    ${fixtureWithNoPkgNameAndNoVersion}

    ${DEPENDENCIES}
    write-json-file ${VERSION_CLR('2.2.0')}

    ${DEV_DEPENDENCIES}
    ${DEV_DEP_ONLY_CLR('is-positive')} ${VERSION_CLR('3.1.0')}

    ${OPTIONAL_DEPENDENCIES}
    ${OPTIONAL_DEP_CLR('is-negative')} ${VERSION_CLR('2.1.0')}
  `)

  t.end()
})

test('list with default parameters in pkg that has no version', async t => {
  t.equal(await list(fixtureWithNoPkgVersion), stripIndent`
    ${LEGEND}

    fixture ${fixtureWithNoPkgVersion}

    ${DEPENDENCIES}
    write-json-file ${VERSION_CLR('2.2.0')}

    ${DEV_DEPENDENCIES}
    ${DEV_DEP_ONLY_CLR('is-positive')} ${VERSION_CLR('3.1.0')}

    ${OPTIONAL_DEPENDENCIES}
    ${OPTIONAL_DEP_CLR('is-negative')} ${VERSION_CLR('2.1.0')}
  `)

  t.end()
})

test('list dev only', async t => {
  t.equal(
    await list(fixture, {
      include: { dependencies: false, devDependencies: true, optionalDependencies: false },
    }),
    stripIndent`
      ${LEGEND}

      fixture@1.0.0 ${fixture}

      ${DEV_DEPENDENCIES}
      ${DEV_DEP_ONLY_CLR('is-positive')} ${VERSION_CLR('3.1.0')}
    `,
  )

  t.end()
})

test('list prod only', async t => {
  t.equal(
    await list(fixture, {
      include: { dependencies: true, devDependencies: false, optionalDependencies: false },
    }),
    stripIndent`
      ${LEGEND}

      fixture@1.0.0 ${fixture}

      ${DEPENDENCIES}
      write-json-file ${VERSION_CLR('2.2.0')}
    `,
  )

  t.end()
})

test('list prod only with depth 2', async t => {
  t.equal(
    await list(fixture, {
      depth: 2,
      include: { dependencies: true, devDependencies: false, optionalDependencies: false },
    }),
    stripIndent`
      ${LEGEND}

      fixture@1.0.0 ${fixture}

      ${DEPENDENCIES}
      write-json-file ${VERSION_CLR('2.2.0')}
      ├── detect-indent ${VERSION_CLR('5.0.0')}
      ├── graceful-fs ${VERSION_CLR('4.1.11')}
      ├─┬ make-dir ${VERSION_CLR('1.0.0')}
      │ └── pify ${VERSION_CLR('2.3.0')}
      ├── pify ${VERSION_CLR('2.3.0')}
      ├─┬ sort-keys ${VERSION_CLR('1.1.2')}
      │ └── is-plain-obj ${VERSION_CLR('1.1.0')}
      └─┬ write-file-atomic ${VERSION_CLR('2.1.0')}
        ├── graceful-fs ${VERSION_CLR('4.1.11')}
        ├── imurmurhash ${VERSION_CLR('0.1.4')}
        └── slide ${VERSION_CLR('1.1.6')}
    `,
  )

  t.end()
})

test('list with depth 1', async t => {
  t.equal(await list(fixture, { depth: 1 }), stripIndent`
    ${LEGEND}

    fixture@1.0.0 ${fixture}

    ${DEPENDENCIES}
    write-json-file ${VERSION_CLR('2.2.0')}
    ├── detect-indent ${VERSION_CLR('5.0.0')}
    ├── graceful-fs ${VERSION_CLR('4.1.11')}
    ├── make-dir ${VERSION_CLR('1.0.0')}
    ├── pify ${VERSION_CLR('2.3.0')}
    ├── sort-keys ${VERSION_CLR('1.1.2')}
    └── write-file-atomic ${VERSION_CLR('2.1.0')}

    ${DEV_DEPENDENCIES}
    ${DEV_DEP_ONLY_CLR('is-positive')} ${VERSION_CLR('3.1.0')}

    ${OPTIONAL_DEPENDENCIES}
    ${OPTIONAL_DEP_CLR('is-negative')} ${VERSION_CLR('2.1.0')}
  `)

  t.end()
})

test('list with depth -1', async t => {
  t.equal(await list(fixture, { depth: -1 }), `fixture@1.0.0 ${fixture}`)

  t.end()
})

test('list with depth 1 and selected packages', async t => {
  t.equal(await listForPackages(['make-dir', 'pify@2', 'sort-keys@2', 'is-negative'], fixture, { depth: 1 }), stripIndent`
    ${LEGEND}

    fixture@1.0.0 ${fixture}

    ${DEPENDENCIES}
    write-json-file ${VERSION_CLR('2.2.0')}
    ├── ${highlighted('make-dir ' + VERSION_CLR('1.0.0'))}
    └── ${highlighted('pify ' + VERSION_CLR('2.3.0'))}

    ${OPTIONAL_DEPENDENCIES}
    ${highlighted(OPTIONAL_DEP_CLR('is-negative') + ' ' + VERSION_CLR('2.1.0'))}
  `)

  t.end()
})

function compareOutputs (t: test.Test, actual: string, expected: string) {
  if (actual !== expected) {
    console.log('Actual:')
    console.log(actual)
    console.log('Expected:')
    console.log(expected)
  }
  t.equal(actual, expected)
}

test('list in long format', async t => {
  compareOutputs(t, await list(fixture, { long: true }), stripIndent`
    ${LEGEND}

    fixture@1.0.0 ${fixture}

    ${DEPENDENCIES}
    write-json-file ${VERSION_CLR('2.2.0')}
      Stringify and write JSON to a file atomically
      git+https://github.com/sindresorhus/write-json-file.git
      https://github.com/sindresorhus/write-json-file#readme

    ${DEV_DEPENDENCIES}
    ${DEV_DEP_ONLY_CLR('is-positive')} ${VERSION_CLR('3.1.0')}
      Check if something is a positive number
      git+https://github.com/kevva/is-positive.git
      https://github.com/kevva/is-positive#readme

    ${OPTIONAL_DEPENDENCIES}
    ${OPTIONAL_DEP_CLR('is-negative')} ${VERSION_CLR('2.1.0')}
      Check if something is a negative number
      git+https://github.com/kevva/is-negative.git
      https://github.com/kevva/is-negative#readme
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
      },
    },
    devDependencies: {
      'is-positive': {
        from: 'is-positive',
        version: '3.1.0',

        resolved: 'https://registry.npmjs.org/is-positive/-/is-positive-3.1.0.tgz',
      },
    },
    optionalDependencies: {
      'is-negative': {
        from: 'is-negative',
        version: '2.1.0',

        resolved: 'https://registry.npmjs.org/is-negative/-/is-negative-2.1.0.tgz',
      },
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
  t.equal(
    await list(fixture, {
      depth: 1,
      include: { dependencies: false, devDependencies: true, optionalDependencies: false },
      reportAs: 'parseable',
    }),
    stripIndent`
      ${fixture}
      ${path.join(fixture, 'node_modules/.registry.npmjs.org/is-positive/3.1.0')}
    `,
  )

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
  t.equal(await list(emptyFixture), `${LEGEND}\n\nempty@1.0.0 ${emptyFixture}`)
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
    {
      unsavedDependencies: [
        {
          alias: 'foo',
          isPeer: false,
          name: 'foo',
          path: '',
          version: '1.0.0',
        },
      ],
    },
    {
      alwaysPrintRootPackage: false,
      depth: 0,
      long: false,
      search: true,
    },
  ), stripIndent`
    ${LEGEND}

    fixture@1.0.0 ${fixture}

    ${UNSAVED_DEPENDENCIES}
    ${NOT_SAVED_DEP_CLR('foo')} ${VERSION_CLR('1.0.0')}
  `)
  t.end()
})

test('write long lists in columns', async (t) => {
  compareOutputs(t, await renderTree(
    {
      name: 'fixture',
      path: fixture,
      version: '1.0.0',
    },
    {
      dependencies: [
        {
          alias: 'a',
          isPeer: false,
          name: 'a',
          path: '',
          version: '1.0.0',
        },
        {
          alias: 'b',
          isPeer: false,
          name: 'b',
          path: '',
          version: '1.0.0',
        },
        {
          alias: 'c',
          isPeer: false,
          name: 'c',
          path: '',
          version: '1.0.0',
        },
        {
          alias: 'd',
          isPeer: false,
          name: 'd',
          path: '',
          version: '1.0.0',
        },
        {
          alias: 'e',
          isPeer: false,
          name: 'e',
          path: '',
          version: '1.0.0',
        },
        {
          alias: 'f',
          isPeer: false,
          name: 'f',
          path: '',
          version: '1.0.0',
        },
        {
          alias: 'g',
          isPeer: false,
          name: 'g',
          path: '',
          version: '1.0.0',
        },
        {
          alias: 'h',
          isPeer: false,
          name: 'h',
          path: '',
          version: '1.0.0',
        },
        {
          alias: 'i',
          isPeer: false,
          name: 'i',
          path: '',
          version: '1.0.0',
        },
        {
          alias: 'k',
          isPeer: false,
          name: 'k',
          path: '',
          version: '1.0.0',
        },
        {
          alias: 'l',
          isPeer: false,
          name: 'l',
          path: '',
          version: '1.0.0',
        },
      ],
    },
    {
      alwaysPrintRootPackage: false,
      depth: 0,
      long: false,
      search: false,
    },
  ), stripIndent`
    ${LEGEND}

    fixture@1.0.0 ${fixture}

    ${DEPENDENCIES}
    a ${VERSION_CLR('1.0.0')}  b ${VERSION_CLR('1.0.0')}  c ${VERSION_CLR('1.0.0')}  d ${VERSION_CLR('1.0.0')}  e ${VERSION_CLR('1.0.0')}  f ${VERSION_CLR('1.0.0')}  g ${VERSION_CLR('1.0.0')}  h ${VERSION_CLR('1.0.0')}  i ${VERSION_CLR('1.0.0')}  k ${VERSION_CLR('1.0.0')}  l ${VERSION_CLR('1.0.0')}
  ` + '  ')
  t.end()
})

test('peer dependencies are marked', async (t) => {
  const fixture = path.join(__dirname, '../../dependencies-hierarchy/fixtures/with-peer')
  const output = await list(fixture, { depth: 1 })
  compareOutputs(t, output, stripIndent`
    ${LEGEND}

    with-peer@1.0.0 ${fixture}

    ${DEPENDENCIES}
    ajv ${VERSION_CLR('6.10.2')}
    ├── fast-deep-equal ${VERSION_CLR('2.0.1')}
    ├── fast-json-stable-stringify ${VERSION_CLR('2.0.0')}
    ├── json-schema-traverse ${VERSION_CLR('0.4.1')}
    └── uri-js ${VERSION_CLR('4.2.2')}
    ajv-keywords ${VERSION_CLR('3.4.1')}
    └── ajv ${VERSION_CLR('6.10.2')} peer
  `)
  t.end()
})
