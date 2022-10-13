import allProjectsAreUpToDate from '../lib/install/allProjectsAreUpToDate'

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
      buildIndex: 0,
      id: 'bar',
      manifest: {
        dependencies: {
          foo: 'workspace:../foo',
        },
      },
      rootDir: 'bar',
    },
    {
      buildIndex: 0,
      id: 'foo',
      manifest: fooManifest,
      rootDir: 'foo',
    },
  ], {
    autoInstallPeers: false,
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
      buildIndex: 0,
      id: 'bar',
      manifest: {
        dependencies: {
          alias: 'npm:foo',
        },
      },
      rootDir: 'bar',
    },
    {
      buildIndex: 0,
      id: 'foo',
      manifest: fooManifest,
      rootDir: 'foo',
    },
  ], {
    autoInstallPeers: false,
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
      buildIndex: 0,
      id: 'bar',
      manifest: {
        dependencies: {
          alias: 'npm:foo@1',
        },
      },
      rootDir: 'bar',
    },
    {
      buildIndex: 0,
      id: 'foo',
      manifest: fooManifest,
      rootDir: 'foo',
    },
  ], {
    autoInstallPeers: false,
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
      buildIndex: 0,
      id: 'bar',
      manifest: {
        dependencies: {
          alias: 'npm:foo@0',
        },
      },
      rootDir: 'bar',
    },
    {
      buildIndex: 0,
      id: 'foo',
      manifest: fooManifest,
      rootDir: 'foo',
    },
  ], {
    autoInstallPeers: false,
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
          buildIndex: 0,
          id: 'bar',
          manifest: {
            dependencies: {
              foo: 'workspace:*',
              foo2: 'workspace:~',
              foo3: 'workspace:^',
            },
          },
          rootDir: 'bar',
        },
        {
          buildIndex: 0,
          id: 'bar2',
          manifest: {
            dependencies: {
              foo: '1.0.0',
            },
          },
          rootDir: 'bar2',
        },
        {
          buildIndex: 0,
          id: 'foo',
          manifest: fooManifest,
          rootDir: 'foo',
        },
        {
          buildIndex: 0,
          id: 'foo2',
          manifest: {
            name: 'foo2',
            version: '1.0.0',
          },
          rootDir: 'foo2',
        },
        {
          buildIndex: 0,
          id: 'foo3',
          manifest: {
            name: 'foo3',
            version: '1.0.0',
          },
          rootDir: 'foo3',
        },
      ],
      {
        autoInstallPeers: false,
        linkWorkspacePackages: false,
        wantedLockfile: {
          importers: {
            bar: {
              dependencies: {
                foo: 'link:../foo',
                foo2: 'link:../foo2',
                foo3: 'link:../foo3',
              },
              specifiers: {
                foo: 'workspace:*',
                foo2: 'workspace:~',
                foo3: 'workspace:^',
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
            foo2: {
              specifiers: {},
            },
            foo3: {
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

test('allProjectsAreUpToDate(): returns false if dependenciesMeta differs', async () => {
  expect(await allProjectsAreUpToDate([
    {
      buildIndex: 0,
      id: 'bar',
      manifest: {
        dependencies: {
          foo: 'workspace:../foo',
        },
        dependenciesMeta: {
          foo: {
            injected: true,
          },
        },
      },
      rootDir: 'bar',
    },
    {
      buildIndex: 0,
      id: 'foo',
      manifest: fooManifest,
      rootDir: 'foo',
    },
  ], {
    autoInstallPeers: false,
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
  })).toBeFalsy()
})

test('allProjectsAreUpToDate(): returns true if dependenciesMeta matches', async () => {
  expect(await allProjectsAreUpToDate([
    {
      buildIndex: 0,
      id: 'bar',
      manifest: {
        dependencies: {
          foo: 'workspace:../foo',
        },
        dependenciesMeta: {
          foo: {
            injected: true,
          },
        },
      },
      rootDir: 'bar',
    },
    {
      buildIndex: 0,
      id: 'foo',
      manifest: fooManifest,
      rootDir: 'foo',
    },
  ], {
    autoInstallPeers: false,
    linkWorkspacePackages: true,
    wantedLockfile: {
      importers: {
        bar: {
          dependencies: {
            foo: 'link:../foo',
          },
          dependenciesMeta: {
            foo: {
              injected: true,
            },
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
