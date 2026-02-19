/// <reference path="../../../__typings__/index.d.ts"/>
import path from 'path'
import { list, listForPackages } from '@pnpm/list'
import { fixtures } from '@pnpm/test-fixtures'
import chalk from 'chalk'
import { renderTree } from '../lib/renderTree.js'
import { renderParseable } from '../lib/renderParseable.js'

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

const highlighted = chalk.bold
const boldHighlighted = highlighted.underline

const f = fixtures(import.meta.dirname)
const fixture = f.find('fixture')
const fixtureWithNoPkgNameAndNoVersion = f.find('fixture-with-no-pkg-name-and-no-version')
const fixtureWithNoPkgVersion = f.find('fixture-with-no-pkg-version')
const fixtureWithExternalLockfile = path.join(f.find('fixture-with-external-shrinkwrap'), 'pkg')
const workspaceWith2Pkgs = f.find('workspace-with-2-pkgs')
const workspaceWithDifferentDeps = f.find('workspace-with-different-deps')
const workspaceWithPrivatePkgs = f.find('workspace-with-private-pkgs')
const emptyFixture = f.find('empty')
const fixtureWithAliasedDep = f.find('with-aliased-dep')

test('list all deps of a package that has an external lockfile', async () => {
  expect(await list([fixtureWithExternalLockfile], {
    lockfileDir: path.join(fixtureWithExternalLockfile, '..'),
    virtualStoreDirMaxLength: 120,
  })).toBe(`${LEGEND}

${boldHighlighted(`pkg@1.0.0 ${fixtureWithExternalLockfile}`)}
│
│   ${DEPENDENCIES}
└── is-positive${VERSION_CLR('@1.0.0')}`)
})

test('print legend only once', async () => {
  expect(await list([
    path.join(workspaceWith2Pkgs, 'packages/bar'),
    path.join(workspaceWith2Pkgs, 'packages/foo'),
  ], {
    lockfileDir: workspaceWith2Pkgs,
    virtualStoreDirMaxLength: 120,
  })).toBe(`${LEGEND}

${boldHighlighted(`bar@0.0.0 ${path.join(workspaceWith2Pkgs, 'packages/bar')}`)}
│
│   ${DEPENDENCIES}
└── is-positive${VERSION_CLR('@1.0.0')}

${boldHighlighted(`foo@0.0.0 ${path.join(workspaceWith2Pkgs, 'packages/foo')}`)}
│
│   ${DEPENDENCIES}
└── is-positive${VERSION_CLR('@1.0.0')}`)
})

test('list in workspace with private package', async () => {
  expect(await list([
    path.join(workspaceWithPrivatePkgs, 'packages/private'),
    path.join(workspaceWithPrivatePkgs, 'packages/public'),
  ], {
    lockfileDir: workspaceWithPrivatePkgs,
    virtualStoreDirMaxLength: 120,
  })).toBe(`${LEGEND}

${boldHighlighted(`private@1.0.0 ${path.join(workspaceWithPrivatePkgs, 'packages/private')} (PRIVATE)`)}
│
│   ${DEPENDENCIES}
└── is-positive${VERSION_CLR('@1.0.0')}

${boldHighlighted(`public@1.0.0 ${path.join(workspaceWithPrivatePkgs, 'packages/public')}`)}
│
│   ${DEPENDENCIES}
└── is-positive${VERSION_CLR('@1.0.0')}`)
})

test('list with default parameters', async () => {
  expect(await list([fixture], { lockfileDir: fixture, virtualStoreDirMaxLength: 120 })).toBe(`${LEGEND}

${boldHighlighted(`fixture@1.0.0 ${fixture}`)}
│
│   ${DEPENDENCIES}
├── write-json-file${VERSION_CLR('@2.3.0')}
│
│   ${DEV_DEPENDENCIES}
├── ${DEV_DEP_ONLY_CLR('is-positive')}${VERSION_CLR('@3.1.0')}
│
│   ${OPTIONAL_DEPENDENCIES}
└── ${OPTIONAL_DEP_CLR('is-negative')}${VERSION_CLR('@2.1.0')}`)
})

test('list with default parameters in pkg that has no name and version', async () => {
  expect(await list([fixtureWithNoPkgNameAndNoVersion], { lockfileDir: fixtureWithNoPkgNameAndNoVersion, virtualStoreDirMaxLength: 120 })).toBe(`${LEGEND}

${boldHighlighted(fixtureWithNoPkgNameAndNoVersion)}
│
│   ${DEPENDENCIES}
├── write-json-file${VERSION_CLR('@2.3.0')}
│
│   ${DEV_DEPENDENCIES}
├── ${DEV_DEP_ONLY_CLR('is-positive')}${VERSION_CLR('@3.1.0')}
│
│   ${OPTIONAL_DEPENDENCIES}
└── ${OPTIONAL_DEP_CLR('is-negative')}${VERSION_CLR('@2.1.0')}`)
})

test('list with default parameters in pkg that has no version', async () => {
  expect(await list([fixtureWithNoPkgVersion], { lockfileDir: fixtureWithNoPkgVersion, virtualStoreDirMaxLength: 120 })).toBe(`${LEGEND}

${boldHighlighted(`fixture ${fixtureWithNoPkgVersion}`)}
│
│   ${DEPENDENCIES}
├── write-json-file${VERSION_CLR('@2.3.0')}
│
│   ${DEV_DEPENDENCIES}
├── ${DEV_DEP_ONLY_CLR('is-positive')}${VERSION_CLR('@3.1.0')}
│
│   ${OPTIONAL_DEPENDENCIES}
└── ${OPTIONAL_DEP_CLR('is-negative')}${VERSION_CLR('@2.1.0')}`)
})

test('list dev only', async () => {
  expect(
    await list([fixture], {
      include: { dependencies: false, devDependencies: true, optionalDependencies: false },
      lockfileDir: fixture,
      virtualStoreDirMaxLength: 120,
    })
  ).toBe(`${LEGEND}

${boldHighlighted(`fixture@1.0.0 ${fixture}`)}
│
│   ${DEV_DEPENDENCIES}
└── ${DEV_DEP_ONLY_CLR('is-positive')}${VERSION_CLR('@3.1.0')}`
  )
})

test('list prod only', async () => {
  expect(
    await list([fixture], {
      include: { dependencies: true, devDependencies: false, optionalDependencies: false },
      lockfileDir: fixture,
      virtualStoreDirMaxLength: 120,
    })
  ).toBe(`${LEGEND}

${boldHighlighted(`fixture@1.0.0 ${fixture}`)}
│
│   ${DEPENDENCIES}
└── write-json-file${VERSION_CLR('@2.3.0')}`
  )
})

test('list prod only with depth 2', async () => {
  expect(
    await list([fixture], {
      depth: 2,
      include: { dependencies: true, devDependencies: false, optionalDependencies: false },
      lockfileDir: fixture,
      virtualStoreDirMaxLength: 120,
    })
  ).toBe(`${LEGEND}

${boldHighlighted(`fixture@1.0.0 ${fixture}`)}
│
│   ${DEPENDENCIES}
└─┬ write-json-file${VERSION_CLR('@2.3.0')}
  ├── detect-indent${VERSION_CLR('@5.0.0')}
  ├── graceful-fs${VERSION_CLR('@4.2.2')}
  ├─┬ make-dir${VERSION_CLR('@1.3.0')}
  │ └── pify${VERSION_CLR('@3.0.0')}
  ├── pify${VERSION_CLR('@3.0.0')}
  ├─┬ sort-keys${VERSION_CLR('@2.0.0')}
  │ └── is-plain-obj${VERSION_CLR('@1.1.0')}
  └─┬ write-file-atomic${VERSION_CLR('@2.4.3')}
    ├── graceful-fs${VERSION_CLR('@4.2.2')}
    ├── imurmurhash${VERSION_CLR('@0.1.4')}
    └── signal-exit${VERSION_CLR('@3.0.2')}`
  )
})

test('list with depth 1', async () => {
  expect(await list([fixture], { depth: 1, lockfileDir: fixture, virtualStoreDirMaxLength: 120 })).toBe(`${LEGEND}

${boldHighlighted(`fixture@1.0.0 ${fixture}`)}
│
│   ${DEPENDENCIES}
├─┬ write-json-file${VERSION_CLR('@2.3.0')}
│ ├── detect-indent${VERSION_CLR('@5.0.0')}
│ ├── graceful-fs${VERSION_CLR('@4.2.2')}
│ ├── make-dir${VERSION_CLR('@1.3.0')}
│ ├── pify${VERSION_CLR('@3.0.0')}
│ ├── sort-keys${VERSION_CLR('@2.0.0')}
│ └── write-file-atomic${VERSION_CLR('@2.4.3')}
│
│   ${DEV_DEPENDENCIES}
├── ${DEV_DEP_ONLY_CLR('is-positive')}${VERSION_CLR('@3.1.0')}
│
│   ${OPTIONAL_DEPENDENCIES}
└── ${OPTIONAL_DEP_CLR('is-negative')}${VERSION_CLR('@2.1.0')}`)
})

test('list with depth -1', async () => {
  expect(await list([fixture], { depth: -1, lockfileDir: fixture, virtualStoreDirMaxLength: 120 })).toBe(`${boldHighlighted(`fixture@1.0.0 ${fixture}`)}`)
})

test('list with depth 1 and selected packages', async () => {
  expect(
    await listForPackages(['make-dir', 'pify@2', 'sort-keys@2', 'is-negative'], [fixture], { depth: 1, lockfileDir: fixture, virtualStoreDirMaxLength: 120 })
  ).toBe(`${LEGEND}

${boldHighlighted(`fixture@1.0.0 ${fixture}`)}
│
│   ${DEPENDENCIES}
├─┬ write-json-file${VERSION_CLR('@2.3.0')}
│ ├── ${highlighted('make-dir' + VERSION_CLR('@1.3.0'))}
│ └── ${highlighted('sort-keys' + VERSION_CLR('@2.0.0'))}
│
│   ${OPTIONAL_DEPENDENCIES}
└── ${highlighted(OPTIONAL_DEP_CLR('is-negative') + VERSION_CLR('@2.1.0'))}`
  )
})

test('list in long format', async () => {
  expect(await list([fixture], { long: true, lockfileDir: fixture, virtualStoreDirMaxLength: 0 })).toBe(`${LEGEND}

${boldHighlighted(`fixture@1.0.0 ${fixture}`)}
│
│   ${DEPENDENCIES}
├── write-json-file${VERSION_CLR('@2.3.0')}
│   Stringify and write JSON to a file atomically
│   git+https://github.com/sindresorhus/write-json-file.git
│   https://github.com/sindresorhus/write-json-file#readme
│   ${path.join(fixture, 'node_modules/.pnpm/write-json-file@2.3.0/node_modules/write-json-file')}
│
│   ${DEV_DEPENDENCIES}
├── ${DEV_DEP_ONLY_CLR('is-positive')}${VERSION_CLR('@3.1.0')}
│   Check if something is a positive number
│   git+https://github.com/kevva/is-positive.git
│   https://github.com/kevva/is-positive#readme
│   ${path.join(fixture, 'node_modules/.pnpm/is-positive@3.1.0/node_modules/is-positive')}
│
│   ${OPTIONAL_DEPENDENCIES}
└── ${OPTIONAL_DEP_CLR('is-negative')}${VERSION_CLR('@2.1.0')}
    Check if something is a negative number
    git+https://github.com/kevva/is-negative.git
    https://github.com/kevva/is-negative#readme
    ${path.join(fixture, 'node_modules/.pnpm/is-negative@2.1.0/node_modules/is-negative')}`)
})

test('parseable list in workspace with private package', async () => {
  expect(await list([
    path.join(workspaceWithPrivatePkgs, 'packages/private'),
    path.join(workspaceWithPrivatePkgs, 'packages/public'),
  ], {
    reportAs: 'parseable',
    lockfileDir: workspaceWithPrivatePkgs,
    virtualStoreDirMaxLength: 120,
  })).toBe(`${path.join(workspaceWithPrivatePkgs, 'packages/private')}
${path.join(workspaceWithPrivatePkgs, 'node_modules/.pnpm/is-positive@1.0.0/node_modules/is-positive')}
${path.join(workspaceWithPrivatePkgs, 'packages/public')}`)
})

test('long parseable list in workspace with private package', async () => {
  expect(await list([
    path.join(workspaceWithPrivatePkgs, 'packages/private'),
    path.join(workspaceWithPrivatePkgs, 'packages/public'),
  ], {
    reportAs: 'parseable',
    long: true,
    lockfileDir: workspaceWithPrivatePkgs,
    virtualStoreDirMaxLength: 120,
  })).toBe(`${path.join(workspaceWithPrivatePkgs, 'packages/private')}:private@1.0.0:PRIVATE
${path.join(workspaceWithPrivatePkgs, 'node_modules/.pnpm/is-positive@1.0.0/node_modules/is-positive')}:is-positive@1.0.0
${path.join(workspaceWithPrivatePkgs, 'packages/public')}:public@1.0.0`)
})

test('JSON list in workspace with private package', async () => {
  expect(await list([
    path.join(workspaceWithPrivatePkgs, 'packages/private'),
    path.join(workspaceWithPrivatePkgs, 'packages/public'),
  ], {
    reportAs: 'json',
    lockfileDir: workspaceWithPrivatePkgs,
    virtualStoreDirMaxLength: 120,
  })).toBe(
    JSON.stringify([
      {
        name: 'private',
        version: '1.0.0',
        path: path.join(workspaceWithPrivatePkgs, 'packages/private'),
        private: true,
        dependencies: {
          'is-positive': {
            from: 'is-positive',
            version: '1.0.0',
            resolved: 'https://registry.npmjs.org/is-positive/-/is-positive-1.0.0.tgz',
            path: path.join(workspaceWithPrivatePkgs, 'node_modules/.pnpm/is-positive@1.0.0/node_modules/is-positive'),
          },
        },
      },
      {
        name: 'public',
        version: '1.0.0',
        path: path.join(workspaceWithPrivatePkgs, 'packages/public'),
        private: false,
        dependencies: {
          'is-positive': {
            from: 'is-positive',
            version: '1.0.0',
            resolved: 'https://registry.npmjs.org/is-positive/-/is-positive-1.0.0.tgz',
            path: path.join(workspaceWithPrivatePkgs, 'node_modules/.pnpm/is-positive@1.0.0/node_modules/is-positive'),
          },
        },
      },
    ], null, 2)
  )
})

test('parseable list with depth 1', async () => {
  expect(await list([fixture], { reportAs: 'parseable', depth: 1, lockfileDir: fixture, virtualStoreDirMaxLength: 120 })).toBe(`${fixture}
${path.join(fixture, 'node_modules/.pnpm/detect-indent@5.0.0/node_modules/detect-indent')}
${path.join(fixture, 'node_modules/.pnpm/graceful-fs@4.2.2/node_modules/graceful-fs')}
${path.join(fixture, 'node_modules/.pnpm/is-negative@2.1.0/node_modules/is-negative')}
${path.join(fixture, 'node_modules/.pnpm/is-positive@3.1.0/node_modules/is-positive')}
${path.join(fixture, 'node_modules/.pnpm/make-dir@1.3.0/node_modules/make-dir')}
${path.join(fixture, 'node_modules/.pnpm/pify@3.0.0/node_modules/pify')}
${path.join(fixture, 'node_modules/.pnpm/sort-keys@2.0.0/node_modules/sort-keys')}
${path.join(fixture, 'node_modules/.pnpm/write-file-atomic@2.4.3/node_modules/write-file-atomic')}
${path.join(fixture, 'node_modules/.pnpm/write-json-file@2.3.0/node_modules/write-json-file')}`)
})

test('JSON list with depth 1', async () => {
  expect(await list([fixture], { reportAs: 'json', depth: 1, lockfileDir: fixture, virtualStoreDirMaxLength: 120 })).toBe(JSON.stringify([{
    name: 'fixture',
    version: '1.0.0',
    path: fixture,
    private: false,
    dependencies: {
      'write-json-file': {
        from: 'write-json-file',
        version: '2.3.0',

        resolved: 'https://registry.npmjs.org/write-json-file/-/write-json-file-2.3.0.tgz',
        path: path.join(fixture, 'node_modules/.pnpm/write-json-file@2.3.0/node_modules/write-json-file'),

        dependencies: {
          'detect-indent': {
            from: 'detect-indent',
            version: '5.0.0',

            resolved: 'https://registry.npmjs.org/detect-indent/-/detect-indent-5.0.0.tgz',
            path: path.join(fixture, 'node_modules/.pnpm/detect-indent@5.0.0/node_modules/detect-indent'),
          },
          'graceful-fs': {
            from: 'graceful-fs',
            version: '4.2.2',

            resolved: 'https://registry.npmjs.org/graceful-fs/-/graceful-fs-4.2.2.tgz',
            path: path.join(fixture, 'node_modules/.pnpm/graceful-fs@4.2.2/node_modules/graceful-fs'),
          },
          'make-dir': {
            from: 'make-dir',
            version: '1.3.0',

            resolved: 'https://registry.npmjs.org/make-dir/-/make-dir-1.3.0.tgz',
            path: path.join(fixture, 'node_modules/.pnpm/make-dir@1.3.0/node_modules/make-dir'),
          },
          pify: {
            from: 'pify',
            version: '3.0.0',

            resolved: 'https://registry.npmjs.org/pify/-/pify-3.0.0.tgz',
            path: path.join(fixture, 'node_modules/.pnpm/pify@3.0.0/node_modules/pify'),
          },
          'sort-keys': {
            from: 'sort-keys',
            version: '2.0.0',

            resolved: 'https://registry.npmjs.org/sort-keys/-/sort-keys-2.0.0.tgz',
            path: path.join(fixture, 'node_modules/.pnpm/sort-keys@2.0.0/node_modules/sort-keys'),
          },
          'write-file-atomic': {
            from: 'write-file-atomic',
            version: '2.4.3',

            resolved: 'https://registry.npmjs.org/write-file-atomic/-/write-file-atomic-2.4.3.tgz',
            path: path.join(fixture, 'node_modules/.pnpm/write-file-atomic@2.4.3/node_modules/write-file-atomic'),
          },
        },
      },
    },
    devDependencies: {
      'is-positive': {
        from: 'is-positive',
        version: '3.1.0',

        resolved: 'https://registry.npmjs.org/is-positive/-/is-positive-3.1.0.tgz',
        path: path.join(fixture, 'node_modules/.pnpm/is-positive@3.1.0/node_modules/is-positive'),
      },
    },
    optionalDependencies: {
      'is-negative': {
        from: 'is-negative',
        version: '2.1.0',

        resolved: 'https://registry.npmjs.org/is-negative/-/is-negative-2.1.0.tgz',
        path: path.join(fixture, 'node_modules/.pnpm/is-negative@2.1.0/node_modules/is-negative'),
      },
    },
  }], null, 2))
})

test('JSON list with aliased dep', async () => {
  expect(
    await list([fixtureWithAliasedDep], { reportAs: 'json', lockfileDir: fixtureWithAliasedDep, virtualStoreDirMaxLength: 120 })
  ).toBe(
    JSON.stringify([
      {
        name: 'with-aliased-dep',
        version: '1.0.0',
        path: fixtureWithAliasedDep,
        private: false,
        dependencies: {
          positive: {
            from: 'is-positive',
            version: '1.0.0',
            resolved: 'https://registry.npmjs.org/is-positive/-/is-positive-1.0.0.tgz',
            path: path.join(fixtureWithAliasedDep, 'node_modules/.pnpm/is-positive@1.0.0/node_modules/is-positive'),
          },
        },
      },
    ], null, 2)
  )
  expect(
    await list([fixtureWithAliasedDep], { lockfileDir: fixtureWithAliasedDep, long: true, reportAs: 'json', virtualStoreDirMaxLength: 120 })
  ).toBe(
    JSON.stringify([{
      name: 'with-aliased-dep',
      version: '1.0.0',
      path: fixtureWithAliasedDep,
      private: false,
      dependencies: {
        positive: {
          from: 'is-positive',
          version: '1.0.0',

          resolved: 'https://registry.npmjs.org/is-positive/-/is-positive-1.0.0.tgz',

          description: 'Test if a number is positive',
          license: 'MIT',
          author: {
            name: 'Kevin Martensson',
            email: 'kevinmartensson@gmail.com',
            url: 'github.com/kevva',
          },
          homepage: 'https://github.com/kevva/is-positive#readme',
          repository: 'git+https://github.com/kevva/is-positive.git',
          path: path.join(fixtureWithAliasedDep, 'node_modules/.pnpm/is-positive@1.0.0/node_modules/is-positive'),
        },
      },
    }], null, 2)
  )
})

test('parseable list with depth 1 and dev only', async () => {
  expect(
    await list([fixture], {
      depth: 1,
      include: { dependencies: false, devDependencies: true, optionalDependencies: false },
      lockfileDir: fixture,
      reportAs: 'parseable',
      virtualStoreDirMaxLength: 120,
    })
  ).toBe(`${fixture}
${path.join(fixture, 'node_modules/.pnpm/is-positive@3.1.0/node_modules/is-positive')}`
  )
})

test('parseable list with depth 1 without unnecessary empty newlines', async () => {
  expect(await listForPackages(
    ['is-positive'],
    [
      path.join(workspaceWithDifferentDeps, 'packages/bar'),
      path.join(workspaceWithDifferentDeps, 'packages/foo'),
    ], {
      alwaysPrintRootPackage: false,
      lockfileDir: workspaceWithDifferentDeps,
      depth: 1,
      reportAs: 'parseable',
      virtualStoreDirMaxLength: 120,
    }
  )).toBe(`${path.join(workspaceWithDifferentDeps, 'packages/bar')}
${path.join(workspaceWithDifferentDeps, 'node_modules/.pnpm/is-positive@3.1.0/node_modules/is-positive')}`
  )
})

test('long parseable list with depth 1', async () => {
  expect(await list([fixture], { reportAs: 'parseable', depth: 1, lockfileDir: fixture, long: true, virtualStoreDirMaxLength: 120 })).toBe(`${fixture}:fixture@1.0.0
${path.join(fixture, 'node_modules/.pnpm/detect-indent@5.0.0/node_modules/detect-indent')}:detect-indent@5.0.0
${path.join(fixture, 'node_modules/.pnpm/graceful-fs@4.2.2/node_modules/graceful-fs')}:graceful-fs@4.2.2
${path.join(fixture, 'node_modules/.pnpm/is-negative@2.1.0/node_modules/is-negative')}:is-negative@2.1.0
${path.join(fixture, 'node_modules/.pnpm/is-positive@3.1.0/node_modules/is-positive')}:is-positive@3.1.0
${path.join(fixture, 'node_modules/.pnpm/make-dir@1.3.0/node_modules/make-dir')}:make-dir@1.3.0
${path.join(fixture, 'node_modules/.pnpm/pify@3.0.0/node_modules/pify')}:pify@3.0.0
${path.join(fixture, 'node_modules/.pnpm/sort-keys@2.0.0/node_modules/sort-keys')}:sort-keys@2.0.0
${path.join(fixture, 'node_modules/.pnpm/write-file-atomic@2.4.3/node_modules/write-file-atomic')}:write-file-atomic@2.4.3
${path.join(fixture, 'node_modules/.pnpm/write-json-file@2.3.0/node_modules/write-json-file')}:write-json-file@2.3.0`)
})

test('long parseable list with depth 1 when package has no version', async () => {
  expect(await list([fixtureWithNoPkgVersion], { reportAs: 'parseable', depth: 1, lockfileDir: fixtureWithNoPkgVersion, long: true, virtualStoreDirMaxLength: 120 })).toBe(`\
${fixtureWithNoPkgVersion}:fixture
${path.join(fixtureWithNoPkgVersion, 'node_modules/.pnpm/detect-indent@5.0.0/node_modules/detect-indent')}:detect-indent@5.0.0
${path.join(fixtureWithNoPkgVersion, 'node_modules/.pnpm/graceful-fs@4.2.2/node_modules/graceful-fs')}:graceful-fs@4.2.2
${path.join(fixtureWithNoPkgVersion, 'node_modules/.pnpm/is-negative@2.1.0/node_modules/is-negative')}:is-negative@2.1.0
${path.join(fixtureWithNoPkgVersion, 'node_modules/.pnpm/is-positive@3.1.0/node_modules/is-positive')}:is-positive@3.1.0
${path.join(fixtureWithNoPkgVersion, 'node_modules/.pnpm/make-dir@1.3.0/node_modules/make-dir')}:make-dir@1.3.0
${path.join(fixtureWithNoPkgVersion, 'node_modules/.pnpm/pify@3.0.0/node_modules/pify')}:pify@3.0.0
${path.join(fixtureWithNoPkgVersion, 'node_modules/.pnpm/sort-keys@2.0.0/node_modules/sort-keys')}:sort-keys@2.0.0
${path.join(fixtureWithNoPkgVersion, 'node_modules/.pnpm/write-file-atomic@2.4.3/node_modules/write-file-atomic')}:write-file-atomic@2.4.3
${path.join(fixtureWithNoPkgVersion, 'node_modules/.pnpm/write-json-file@2.3.0/node_modules/write-json-file')}:write-json-file@2.3.0`)
})

test('long parseable list with depth 1 when package has no name and no version', async () => {
  expect(
    await list(
      [fixtureWithNoPkgNameAndNoVersion],
      { reportAs: 'parseable', depth: 1, lockfileDir: fixtureWithNoPkgNameAndNoVersion, long: true, virtualStoreDirMaxLength: 120 }
    )
  ).toBe(`${fixtureWithNoPkgNameAndNoVersion}
${path.join(fixtureWithNoPkgNameAndNoVersion, 'node_modules/.pnpm/detect-indent@5.0.0/node_modules/detect-indent')}:detect-indent@5.0.0
${path.join(fixtureWithNoPkgNameAndNoVersion, 'node_modules/.pnpm/graceful-fs@4.2.2/node_modules/graceful-fs')}:graceful-fs@4.2.2
${path.join(fixtureWithNoPkgNameAndNoVersion, 'node_modules/.pnpm/is-negative@2.1.0/node_modules/is-negative')}:is-negative@2.1.0
${path.join(fixtureWithNoPkgNameAndNoVersion, 'node_modules/.pnpm/is-positive@3.1.0/node_modules/is-positive')}:is-positive@3.1.0
${path.join(fixtureWithNoPkgNameAndNoVersion, 'node_modules/.pnpm/make-dir@1.3.0/node_modules/make-dir')}:make-dir@1.3.0
${path.join(fixtureWithNoPkgNameAndNoVersion, 'node_modules/.pnpm/pify@3.0.0/node_modules/pify')}:pify@3.0.0
${path.join(fixtureWithNoPkgNameAndNoVersion, 'node_modules/.pnpm/sort-keys@2.0.0/node_modules/sort-keys')}:sort-keys@2.0.0
${path.join(fixtureWithNoPkgNameAndNoVersion, 'node_modules/.pnpm/write-file-atomic@2.4.3/node_modules/write-file-atomic')}:write-file-atomic@2.4.3
${path.join(fixtureWithNoPkgNameAndNoVersion, 'node_modules/.pnpm/write-json-file@2.3.0/node_modules/write-json-file')}:write-json-file@2.3.0`
  )
})

test('print empty', async () => {
  expect(await list([emptyFixture], { lockfileDir: emptyFixture, virtualStoreDirMaxLength: 120 })).toBe(`${LEGEND}\n\n${boldHighlighted(`empty@1.0.0 ${emptyFixture}`)}`)
})

test("don't print empty", async () => {
  expect(await list([emptyFixture], { alwaysPrintRootPackage: false, lockfileDir: emptyFixture, virtualStoreDirMaxLength: 120 })).toBe('')
})

test('unsaved dependencies are marked', async () => {
  expect(await renderTree(
    [
      {
        name: 'fixture',
        path: fixture,
        version: '1.0.0',

        unsavedDependencies: [
          {
            alias: 'foo',
            isMissing: false,
            isPeer: false,
            isSkipped: false,
            name: 'foo',
            path: '',
            version: '1.0.0',
          },
        ],
      },
    ],
    {
      alwaysPrintRootPackage: false,
      depth: 0,
      long: false,
      search: true,
      showExtraneous: true,
    }
  )).toBe(`${LEGEND}

${boldHighlighted(`fixture@1.0.0 ${fixture}`)}
│
│   ${UNSAVED_DEPENDENCIES}
└── ${NOT_SAVED_DEP_CLR('foo')}${VERSION_CLR('@1.0.0')}`)
})

test('list with many dependencies', async () => {
  expect(await renderTree(
    [
      {
        name: 'fixture',
        path: fixture,
        version: '1.0.0',

        dependencies: [
          {
            alias: 'a',
            isMissing: false,
            isPeer: false,
            isSkipped: false,
            name: 'a',
            path: '',
            version: '1.0.0',
          },
          {
            alias: 'b',
            isMissing: false,
            isPeer: false,
            isSkipped: false,
            name: 'b',
            path: '',
            version: '1.0.0',
          },
          {
            alias: 'c',
            isMissing: false,
            isPeer: false,
            isSkipped: false,
            name: 'c',
            path: '',
            version: '1.0.0',
          },
          {
            alias: 'd',
            isMissing: false,
            isPeer: false,
            isSkipped: false,
            name: 'd',
            path: '',
            version: '1.0.0',
          },
          {
            alias: 'e',
            isMissing: false,
            isPeer: false,
            isSkipped: false,
            name: 'e',
            path: '',
            version: '1.0.0',
          },
          {
            alias: 'f',
            isMissing: false,
            isPeer: false,
            isSkipped: false,
            name: 'f',
            path: '',
            version: '1.0.0',
          },
          {
            alias: 'g',
            isMissing: false,
            isPeer: false,
            isSkipped: false,
            name: 'g',
            path: '',
            version: '1.0.0',
          },
          {
            alias: 'h',
            isMissing: false,
            isPeer: false,
            isSkipped: false,
            name: 'h',
            path: '',
            version: '1.0.0',
          },
          {
            alias: 'i',
            isMissing: false,
            isPeer: false,
            isSkipped: false,
            name: 'i',
            path: '',
            version: '1.0.0',
          },
          {
            alias: 'k',
            isMissing: false,
            isPeer: false,
            isSkipped: false,
            name: 'k',
            path: '',
            version: '1.0.0',
          },
          {
            alias: 'l',
            isMissing: false,
            isPeer: false,
            isSkipped: false,
            name: 'l',
            path: '',
            version: '1.0.0',
          },
        ],
      },
    ],
    {
      alwaysPrintRootPackage: false,
      depth: 0,
      long: false,
      search: false,
      showExtraneous: false,
    }
  )).toBe(`${LEGEND}

${boldHighlighted(`fixture@1.0.0 ${fixture}`)}
│
│   ${DEPENDENCIES}
├── a${VERSION_CLR('@1.0.0')}
├── b${VERSION_CLR('@1.0.0')}
├── c${VERSION_CLR('@1.0.0')}
├── d${VERSION_CLR('@1.0.0')}
├── e${VERSION_CLR('@1.0.0')}
├── f${VERSION_CLR('@1.0.0')}
├── g${VERSION_CLR('@1.0.0')}
├── h${VERSION_CLR('@1.0.0')}
├── i${VERSION_CLR('@1.0.0')}
├── k${VERSION_CLR('@1.0.0')}
└── l${VERSION_CLR('@1.0.0')}`)
})

test('sort list items', async () => {
  expect(await renderTree(
    [
      {
        name: 'fixture',
        path: fixture,
        version: '1.0.0',

        dependencies: [
          {
            alias: 'foo',
            isMissing: false,
            isPeer: false,
            isSkipped: false,
            name: 'foo',
            path: '',
            version: '1.0.0',

            dependencies: [
              {
                alias: 'qar',
                isMissing: false,
                isPeer: false,
                isSkipped: false,
                name: 'qar',
                path: '',
                version: '1.0.0',
              },
              {
                alias: 'bar',
                isMissing: false,
                isPeer: false,
                isSkipped: false,
                name: 'bar',
                path: '',
                version: '1.0.0',
              },
            ],
          },
        ],
      },
    ],
    {
      alwaysPrintRootPackage: false,
      depth: 0,
      long: false,
      search: false,
      showExtraneous: false,
    }
  )).toBe(`${LEGEND}

${boldHighlighted(`fixture@1.0.0 ${fixture}`)}
│
│   ${DEPENDENCIES}
└─┬ foo${VERSION_CLR('@1.0.0')}
  ├── bar${VERSION_CLR('@1.0.0')}
  └── qar${VERSION_CLR('@1.0.0')}`)
})

test('peer dependencies are marked', async () => {
  const fixture = f.find('with-peer')
  const output = await list([fixture], { depth: 1, lockfileDir: fixture, virtualStoreDirMaxLength: 120 })
  expect(output).toBe(`${LEGEND}

${boldHighlighted(`with-peer@1.0.0 ${fixture}`)}
│
│   ${DEPENDENCIES}
├─┬ ajv${VERSION_CLR('@6.10.2')}
│ ├── fast-deep-equal${VERSION_CLR('@2.0.1')}
│ ├── fast-json-stable-stringify${VERSION_CLR('@2.0.0')}
│ ├── json-schema-traverse${VERSION_CLR('@0.4.1')}
│ └── uri-js${VERSION_CLR('@4.2.2')}
└─┬ ajv-keywords${VERSION_CLR('@3.4.1')}
  └── ajv${VERSION_CLR('@6.10.2')} peer`)
})

test('peer dependencies are marked when searching', async () => {
  const fixture = f.find('with-peer')
  const output = await listForPackages(['ajv'], [fixture], { depth: 1, lockfileDir: fixture, virtualStoreDirMaxLength: 120 })
  expect(output).toBe(`${LEGEND}

${boldHighlighted(`with-peer@1.0.0 ${fixture}`)}
│
│   ${DEPENDENCIES}
├── ${highlighted(`ajv${VERSION_CLR('@6.10.2')}`)}
└─┬ ajv-keywords${VERSION_CLR('@3.4.1')}
  └── ${highlighted(`ajv${VERSION_CLR('@6.10.2')} peer`)}`)
})

test('--only-projects shows only projects', async () => {
  const fixture = f.find('workspace-with-nested-workspace-deps')
  const output = await list([fixture], { depth: 999, lockfileDir: fixture, onlyProjects: true, virtualStoreDirMaxLength: 120 })

  // The "workspace-with-nested-workspace-deps" test case has an external
  // dependency under @scope/b, but that package should not be printed when
  // --only-projects is passed to the list command.
  expect(output).toBe(`${LEGEND}

${boldHighlighted(`root@1.0.0 ${fixture}`)}
│
│   ${DEPENDENCIES}
└─┬ @scope/a${VERSION_CLR('@link:packages/a')}
  └─┬ @scope/b${VERSION_CLR('@link:packages/b')}
    └── @scope/c${VERSION_CLR('@link:packages/c')}`)
})

test('renderTree displays npm: protocol for aliased packages', async () => {
  const testPath = '/test/path'
  const output = await renderTree(
    [
      {
        name: 'test-project',
        path: testPath,
        version: '1.0.0',
        dependencies: [
          {
            alias: 'foo',
            name: '@pnpm.e2e/pkg-with-1-dep',
            version: '100.0.0',
            path: '/test/path/node_modules/.pnpm/@pnpm.e2e+pkg-with-1-dep@100.0.0/node_modules/@pnpm.e2e/pkg-with-1-dep',
            isMissing: false,
            isPeer: false,
            isSkipped: false,
          },
        ],
      },
    ],
    {
      alwaysPrintRootPackage: false,
      depth: 0,
      long: false,
      search: false,
      showExtraneous: false,
    }
  )

  // renderTree uses chalk for coloring, so we check parts separately
  expect(output).toContain('foo')
  expect(output).toContain('npm:@pnpm.e2e/pkg-with-1-dep@100.0.0')
})

test('renderTree displays file: protocol correctly for aliased packages', async () => {
  const testPath = '/test/path'
  const output = await renderTree(
    [
      {
        name: 'test-project',
        path: testPath,
        version: '1.0.0',
        dependencies: [
          {
            alias: 'my-alias',
            name: 'my-local-pkg',
            version: 'my-local-pkg@file:local-pkg',
            path: '/test/path/local-pkg',
            isMissing: false,
            isPeer: false,
            isSkipped: false,
          },
        ],
      },
    ],
    {
      alwaysPrintRootPackage: false,
      depth: 0,
      long: false,
      search: false,
      showExtraneous: false,
    }
  )

  // renderTree uses chalk for coloring, so we check parts separately
  // instead of matching the complete string with color codes
  expect(output).toContain('my-alias')
  expect(output).toContain('my-local-pkg@file:local-pkg')
})

test('renderParseable displays npm: protocol for aliased packages', async () => {
  const testPath = '/test/path'
  const output = await renderParseable(
    [
      {
        name: 'test-project',
        path: testPath,
        version: '1.0.0',
        dependencies: [
          {
            alias: 'foo',
            name: '@pnpm.e2e/pkg-with-1-dep',
            version: '100.0.0',
            path: '/test/path/node_modules/.pnpm/@pnpm.e2e+pkg-with-1-dep@100.0.0/node_modules/@pnpm.e2e/pkg-with-1-dep',
            isMissing: false,
            isPeer: false,
            isSkipped: false,
          },
        ],
      },
    ],
    {
      alwaysPrintRootPackage: false,
      depth: 0,
      long: true,
      search: false,
    }
  )

  expect(output).toContain('foo npm:@pnpm.e2e/pkg-with-1-dep@100.0.0')
})

test('renderParseable displays file: protocol correctly for aliased packages', async () => {
  const testPath = '/test/path'
  const output = await renderParseable(
    [
      {
        name: 'test-project',
        path: testPath,
        version: '1.0.0',
        dependencies: [
          {
            alias: 'my-alias',
            name: 'my-local-pkg',
            version: 'my-local-pkg@file:local-pkg',
            path: '/test/path/local-pkg',
            isMissing: false,
            isPeer: false,
            isSkipped: false,
          },
        ],
      },
    ],
    {
      alwaysPrintRootPackage: false,
      depth: 0,
      long: true,
      search: false,
    }
  )

  expect(output).toContain('my-alias my-local-pkg@file:local-pkg')
})
