import allProjectsAreUpToDate from 'supi/lib/install/allProjectsAreUpToDate'
import tape = require('tape')
import promisifyTape from 'tape-promise'

const test = promisifyTape(tape)

const fooManifest = {
  name: 'foo',
  version: '1.0.0',
}
const workspacePackages = {
  foo: {
    '1.0.0': {
      dir: 'foo',
      manifest: fooManifest,
    },
  },
}

test('allProjectsAreUpToDate(): works with aliased local dependencies', async (t: tape.Test) => {
  t.ok(await allProjectsAreUpToDate([
    {
      id: 'bar',
      manifest: {
        dependencies: {
          alias: 'npm:foo',
        },
      },
      rootDir: 'bar',
    },
    {
      id: 'foo',
      manifest: fooManifest,
      rootDir: 'foo',
    },
  ], {
    wantedLockfile: {
      importers: {
        bar: {
          dependencies: {
            alias: 'link:../foo',
          },
          specifiers: {
            alias: 'npm:foo',
          },
        },
        foo: {
          specifiers: {},
        },
      },
      lockfileVersion: 5,
    },
    workspacePackages,
  }))
})

test('allProjectsAreUpToDate(): works with aliased local dependencies that specify versions', async (t: tape.Test) => {
  t.ok(await allProjectsAreUpToDate([
    {
      id: 'bar',
      manifest: {
        dependencies: {
          alias: 'npm:foo@1',
        },
      },
      rootDir: 'bar',
    },
    {
      id: 'foo',
      manifest: fooManifest,
      rootDir: 'foo',
    },
  ], {
    wantedLockfile: {
      importers: {
        bar: {
          dependencies: {
            alias: 'link:../foo',
          },
          specifiers: {
            alias: 'npm:foo@1',
          },
        },
        foo: {
          specifiers: {},
        },
      },
      lockfileVersion: 5,
    },
    workspacePackages,
  }))
})

test('allProjectsAreUpToDate(): returns false if the aliased dependency version is out of date', async (t: tape.Test) => {
  t.notOk(await allProjectsAreUpToDate([
    {
      id: 'bar',
      manifest: {
        dependencies: {
          alias: 'npm:foo@0',
        },
      },
      rootDir: 'bar',
    },
    {
      id: 'foo',
      manifest: fooManifest,
      rootDir: 'foo',
    },
  ], {
    wantedLockfile: {
      importers: {
        bar: {
          dependencies: {
            alias: 'link:../foo',
          },
          specifiers: {
            alias: 'npm:foo@0',
          },
        },
        foo: {
          specifiers: {},
        },
      },
      lockfileVersion: 5,
    },
    workspacePackages,
  }))
})
