import { LOCKFILE_VERSION } from '@pnpm/constants'
import { prepareEmpty } from '@pnpm/prepare'
import { type ProjectId, type ProjectRootDir } from '@pnpm/types'
import { allProjectsAreUpToDate } from '../lib/install/allProjectsAreUpToDate'
import { writeFile, mkdir } from 'fs/promises'
import { type Lockfile } from '@pnpm/lockfile-file'

const fooManifest = {
  name: 'foo',
  version: '1.0.0',
}
const workspacePackages = new Map([
  ['foo', new Map([
    ['1.0.0', {
      rootDir: 'foo' as ProjectRootDir,
      manifest: fooManifest,
    }],
  ])],
])

test('allProjectsAreUpToDate(): works with packages linked through the workspace protocol using relative path', async () => {
  expect(await allProjectsAreUpToDate([
    {
      id: 'bar' as ProjectId,
      manifest: {
        dependencies: {
          foo: 'workspace:../foo',
        },
      },
      rootDir: 'bar' as ProjectRootDir,
    },
    {
      id: 'foo' as ProjectId,
      manifest: fooManifest,
      rootDir: 'foo' as ProjectRootDir,
    },
  ], {
    autoInstallPeers: false,
    catalogs: {},
    excludeLinksFromLockfile: false,
    linkWorkspacePackages: true,
    wantedLockfile: {
      importers: {
        ['bar' as ProjectId]: {
          dependencies: {
            foo: 'link:../foo',
          },
          specifiers: {
            foo: 'workspace:../foo',
          },
        },
        ['foo' as ProjectId]: {
          specifiers: {},
        },
      },
      lockfileVersion: LOCKFILE_VERSION,
    },
    workspacePackages,
    lockfileDir: '',
  })).toBeTruthy()
})

test('allProjectsAreUpToDate(): works with aliased local dependencies', async () => {
  expect(await allProjectsAreUpToDate([
    {
      id: 'bar' as ProjectId,
      manifest: {
        dependencies: {
          alias: 'npm:foo',
        },
      },
      rootDir: 'bar' as ProjectRootDir,
    },
    {
      id: 'foo' as ProjectId,
      manifest: fooManifest,
      rootDir: 'foo' as ProjectRootDir,
    },
  ], {
    autoInstallPeers: false,
    catalogs: {},
    excludeLinksFromLockfile: false,
    linkWorkspacePackages: true,
    wantedLockfile: {
      importers: {
        ['bar' as ProjectId]: {
          dependencies: {
            alias: 'link:../foo',
          },
          specifiers: {
            alias: 'npm:foo',
          },
        },
        ['foo' as ProjectId]: {
          specifiers: {},
        },
      },
      lockfileVersion: LOCKFILE_VERSION,
    },
    workspacePackages,
    lockfileDir: '',
  })).toBeTruthy()
})

test('allProjectsAreUpToDate(): works with aliased local dependencies that specify versions', async () => {
  expect(await allProjectsAreUpToDate([
    {
      id: 'bar' as ProjectId,
      manifest: {
        dependencies: {
          alias: 'npm:foo@1',
        },
      },
      rootDir: 'bar' as ProjectRootDir,
    },
    {
      id: 'foo' as ProjectId,
      manifest: fooManifest,
      rootDir: 'foo' as ProjectRootDir,
    },
  ], {
    autoInstallPeers: false,
    catalogs: {},
    excludeLinksFromLockfile: false,
    linkWorkspacePackages: true,
    wantedLockfile: {
      importers: {
        ['bar' as ProjectId]: {
          dependencies: {
            alias: 'link:../foo',
          },
          specifiers: {
            alias: 'npm:foo@1',
          },
        },
        ['foo' as ProjectId]: {
          specifiers: {},
        },
      },
      lockfileVersion: LOCKFILE_VERSION,
    },
    workspacePackages,
    lockfileDir: '',
  })).toBeTruthy()
})

test('allProjectsAreUpToDate(): returns false if the aliased dependency version is out of date', async () => {
  expect(await allProjectsAreUpToDate([
    {
      id: 'bar' as ProjectId,
      manifest: {
        dependencies: {
          alias: 'npm:foo@0',
        },
      },
      rootDir: 'bar' as ProjectRootDir,
    },
    {
      id: 'foo' as ProjectId,
      manifest: fooManifest,
      rootDir: 'foo' as ProjectRootDir,
    },
  ], {
    autoInstallPeers: false,
    catalogs: {},
    excludeLinksFromLockfile: false,
    linkWorkspacePackages: true,
    wantedLockfile: {
      importers: {
        ['bar' as ProjectId]: {
          dependencies: {
            alias: 'link:../foo',
          },
          specifiers: {
            alias: 'npm:foo@0',
          },
        },
        ['foo' as ProjectId]: {
          specifiers: {},
        },
      },
      lockfileVersion: LOCKFILE_VERSION,
    },
    workspacePackages,
    lockfileDir: '',
  })).toBeFalsy()
})

test('allProjectsAreUpToDate(): use link and registry version if linkWorkspacePackages = false', async () => {
  expect(
    await allProjectsAreUpToDate(
      [
        {
          id: 'bar' as ProjectId,
          manifest: {
            dependencies: {
              foo: 'workspace:*',
              foo2: 'workspace:~',
              foo3: 'workspace:^',
            },
          },
          rootDir: 'bar' as ProjectRootDir,
        },
        {
          id: 'bar2' as ProjectId,
          manifest: {
            dependencies: {
              foo: '1.0.0',
            },
          },
          rootDir: 'bar2' as ProjectRootDir,
        },
        {
          id: 'foo' as ProjectId,
          manifest: fooManifest,
          rootDir: 'foo' as ProjectRootDir,
        },
        {
          id: 'foo2' as ProjectId,
          manifest: {
            name: 'foo2',
            version: '1.0.0',
          },
          rootDir: 'foo2' as ProjectRootDir,
        },
        {
          id: 'foo3' as ProjectId,
          manifest: {
            name: 'foo3',
            version: '1.0.0',
          },
          rootDir: 'foo3' as ProjectRootDir,
        },
      ],
      {
        autoInstallPeers: false,
        catalogs: {},
        excludeLinksFromLockfile: false,
        linkWorkspacePackages: false,
        wantedLockfile: {
          importers: {
            ['bar' as ProjectId]: {
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
            ['bar2' as ProjectId]: {
              dependencies: {
                foo: '1.0.0',
              },
              specifiers: {
                foo: '1.0.0',
              },
            },
            ['foo' as ProjectId]: {
              specifiers: {},
            },
            ['foo2' as ProjectId]: {
              specifiers: {},
            },
            ['foo3' as ProjectId]: {
              specifiers: {},
            },
          },
          lockfileVersion: LOCKFILE_VERSION,
        },
        workspacePackages,
        lockfileDir: '',
      }
    )
  ).toBeTruthy()
})

test('allProjectsAreUpToDate(): returns false if dependenciesMeta differs', async () => {
  expect(await allProjectsAreUpToDate([
    {
      id: 'bar' as ProjectId,
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
      rootDir: 'bar' as ProjectRootDir,
    },
    {
      id: 'foo' as ProjectId,
      manifest: fooManifest,
      rootDir: 'foo' as ProjectRootDir,
    },
  ], {
    autoInstallPeers: false,
    catalogs: {},
    excludeLinksFromLockfile: false,
    linkWorkspacePackages: true,
    wantedLockfile: {
      importers: {
        ['bar' as ProjectId]: {
          dependencies: {
            foo: 'link:../foo',
          },
          specifiers: {
            foo: 'workspace:../foo',
          },
        },
        ['foo' as ProjectId]: {
          specifiers: {},
        },
      },
      lockfileVersion: LOCKFILE_VERSION,
    },
    workspacePackages,
    lockfileDir: '',
  })).toBeFalsy()
})

test('allProjectsAreUpToDate(): returns true if dependenciesMeta matches', async () => {
  expect(await allProjectsAreUpToDate([
    {
      id: 'bar' as ProjectId,
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
      rootDir: 'bar' as ProjectRootDir,
    },
    {
      id: 'foo' as ProjectId,
      manifest: fooManifest,
      rootDir: 'foo' as ProjectRootDir,
    },
  ], {
    autoInstallPeers: false,
    catalogs: {},
    excludeLinksFromLockfile: false,
    linkWorkspacePackages: true,
    wantedLockfile: {
      importers: {
        ['bar' as ProjectId]: {
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
        ['foo' as ProjectId]: {
          specifiers: {},
        },
      },
      lockfileVersion: LOCKFILE_VERSION,
    },
    workspacePackages,
    lockfileDir: '',
  })).toBeTruthy()
})

describe('local file dependency', () => {
  beforeEach(async () => {
    prepareEmpty()
    await mkdir('local-dir')
    await writeFile('./local-dir/package.json', JSON.stringify({
      name: 'local-dir',
      version: '1.0.0',
      dependencies: {
        'is-positive': '2.0.0',
      },
    }))
  })
  const projects = [
    {
      id: 'bar' as ProjectId,
      manifest: {
        dependencies: {
          local: 'file:./local-dir',
        },
      },
      rootDir: 'bar' as ProjectRootDir,
    },
    {
      id: 'foo' as ProjectId,
      manifest: fooManifest,
      rootDir: 'foo' as ProjectRootDir,
    },
  ]
  const options = {
    autoInstallPeers: false,
    catalogs: {},
    excludeLinksFromLockfile: false,
    linkWorkspacePackages: true,
    wantedLockfile: {
      importers: {
        bar: {
          dependencies: {
            local: 'file:./local-dir',
          },
          specifiers: {
            local: 'file:./local-dir',
          },
        },
        foo: {
          specifiers: {},
        },
      },
      packages: {
        'local@file:./local-dir': {
          resolution: { directory: './local-dir', type: 'directory' },
          version: '1.0.0',
          dependencies: {
            'is-positive': '2.0.0',
          },
          dev: false,
        },
      },
      lockfileVersion: LOCKFILE_VERSION,
    } as Lockfile,
    workspacePackages,
    lockfileDir: process.cwd(),
  }
  test('allProjectsAreUpToDate(): returns true if local file not changed', async () => {
    expect(await allProjectsAreUpToDate(projects, {
      ...options,
      lockfileDir: process.cwd(),
    })).toBeTruthy()
  })

  test('allProjectsAreUpToDate(): returns false if add new dependency to local file', async () => {
    await writeFile('./local-dir/package.json', JSON.stringify({
      name: 'local-dir',
      version: '1.0.0',
      dependencies: {
        'is-positive': '2.0.0',
        'is-odd': '1.0.0',
      },
    }))
    expect(await allProjectsAreUpToDate(projects, {
      ...options,
      lockfileDir: process.cwd(),
    })).toBeFalsy()
  })

  test('allProjectsAreUpToDate(): returns false if update dependency in local file', async () => {
    await writeFile('./local-dir/package.json', JSON.stringify({
      name: 'local-dir',
      version: '1.0.0',
      dependencies: {
        'is-positive': '3.0.0',
      },
    }))
    expect(await allProjectsAreUpToDate(projects, {
      ...options,
      lockfileDir: process.cwd(),
    })).toBeFalsy()
  })

  test('allProjectsAreUpToDate(): returns false if remove dependency in local file', async () => {
    await writeFile('./local-dir/package.json', JSON.stringify({
      name: 'local-dir',
      version: '1.0.0',
      dependencies: {},
    }))
    expect(await allProjectsAreUpToDate(projects, {
      ...options,
      lockfileDir: process.cwd(),
    })).toBeFalsy()
  })
})

test('allProjectsAreUpToDate(): returns true if workspace dependency\'s version type is tag', async () => {
  const projects = [
    {
      id: 'bar' as ProjectId,
      manifest: {
        dependencies: {
          foo: 'unpublished-tag',
        },
      },
      rootDir: 'bar' as ProjectRootDir,
    },
    {
      id: 'foo' as ProjectId,
      manifest: fooManifest,
      rootDir: 'foo' as ProjectRootDir,
    },
  ]
  const options = {
    autoInstallPeers: false,
    catalogs: {},
    excludeLinksFromLockfile: false,
    linkWorkspacePackages: true,
    wantedLockfile: {
      importers: {
        bar: {
          dependencies: {
            foo: 'link:../foo',
          },
          specifiers: {
            foo: 'unpublished-tag',
          },
        },
        foo: {
          specifiers: {},
        },
      },
      lockfileVersion: LOCKFILE_VERSION,
    } as Lockfile,
    workspacePackages,
    lockfileDir: process.cwd(),
  }
  expect(await allProjectsAreUpToDate(projects, {
    ...options,
    lockfileDir: process.cwd(),
  })).toBeTruthy()
})
