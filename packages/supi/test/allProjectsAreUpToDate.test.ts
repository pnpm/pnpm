import allProjectsAreUpToDate from 'supi/lib/install/allProjectsAreUpToDate'

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

test('allProjectsAreUpToDate(): works with packages linked through the workspace protocol using relative path', async () => {
  expect(await allProjectsAreUpToDate([
    {
      id: 'bar',
      manifest: {
        dependencies: {
          foo: 'workspace:../foo',
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
    linkWorkspacePackages: true,
    wantedLockfile: {
      importers: {
        bar: {
          dependencies: {
            foo: 'link:../foo',
          },
          specifiers: {
            foo: 'workspace:../foo',
          },
        },
        foo: {
          specifiers: {},
        },
      },
      lockfileVersion: 5,
    },
    workspacePackages,
  })).toBeTruthy()
})

test('allProjectsAreUpToDate(): works with aliased local dependencies', async () => {
  expect(await allProjectsAreUpToDate([
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
    linkWorkspacePackages: true,
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
  })).toBeTruthy()
})

test('allProjectsAreUpToDate(): works with aliased local dependencies that specify versions', async () => {
  expect(await allProjectsAreUpToDate([
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
    linkWorkspacePackages: true,
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
  })).toBeTruthy()
})

test('allProjectsAreUpToDate(): returns false if the aliased dependency version is out of date', async () => {
  expect(await allProjectsAreUpToDate([
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
    linkWorkspacePackages: true,
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
  })).toBeFalsy()
})

test('allProjectsAreUpToDate(): use link and registry version if linkWorkspacePackages = false', async () => {
  expect(
    await allProjectsAreUpToDate(
      [
        {
          id: 'bar',
          manifest: {
            dependencies: {
              foo: 'workspace:*',
            },
          },
          rootDir: 'bar',
        },
        {
          id: 'bar2',
          manifest: {
            dependencies: {
              foo: '1.0.0',
            },
          },
          rootDir: 'bar2',
        },
        {
          id: 'foo',
          manifest: fooManifest,
          rootDir: 'foo',
        },
      ],
      {
        linkWorkspacePackages: false,
        wantedLockfile: {
          importers: {
            bar: {
              dependencies: {
                foo: 'link:../foo',
              },
              specifiers: {
                foo: 'workspace:*',
              },
            },
            bar2: {
              dependencies: {
                foo: '1.0.0',
              },
              specifiers: {
                foo: '1.0.0',
              },
            },
            foo: {
              specifiers: {},
            },
          },
          lockfileVersion: 5,
        },
        workspacePackages,
      }
    )
  ).toBeTruthy()
})
