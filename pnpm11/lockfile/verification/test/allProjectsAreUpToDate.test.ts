import { createWriteStream } from 'node:fs'
import { mkdir, rm, writeFile } from 'node:fs/promises'
import os from 'node:os'
import path from 'node:path'
import { pipeline } from 'node:stream/promises'

import { beforeEach, describe, expect, test } from '@jest/globals'
import { LOCKFILE_VERSION } from '@pnpm/constants'
import { getTarballIntegrity } from '@pnpm/crypto.hash'
import type { LockfileObject, PackageSnapshot, ProjectSnapshot, TarballResolution } from '@pnpm/lockfile.types'
import { allProjectsAreUpToDate, checkLinkedPackagesAreUpToDate, findPackageTarballIntegrityMismatch, resolveLocalTarballPath } from '@pnpm/lockfile.verification'
import { prepareEmpty } from '@pnpm/prepare'
import type { WorkspacePackages } from '@pnpm/resolving.resolver-base'
import type { DependencyManifest, DepPath, ProjectId, ProjectRootDir, ReadPackageHook } from '@pnpm/types'
import tar from 'tar-stream'

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

test('allProjectsAreUpToDate(): works with aliased workspace: dependencies', async () => {
  expect(await allProjectsAreUpToDate([
    {
      id: 'bar' as ProjectId,
      manifest: {
        dependencies: {
          alias: 'workspace:@scope/foo@*',
        },
      },
      rootDir: 'bar' as ProjectRootDir,
    },
    {
      id: 'foo' as ProjectId,
      manifest: {
        name: '@scope/foo',
        version: '1.0.0',
      },
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
            alias: 'workspace:@scope/foo@*',
          },
        },
        ['foo' as ProjectId]: {
          specifiers: {},
        },
      },
      lockfileVersion: LOCKFILE_VERSION,
    },
    workspacePackages: new Map([
      ['@scope/foo', new Map([
        ['1.0.0', {
          id: 'foo' as ProjectId,
          manifest: {
            name: '@scope/foo',
            version: '1.0.0',
          },
          rootDir: 'foo' as ProjectRootDir,
        }],
      ])],
    ]),
    lockfileDir: '',
  })).toBeTruthy()
})

test('allProjectsAreUpToDate(): returns false if aliased workspace: dependency version is out of date', async () => {
  expect(await allProjectsAreUpToDate([
    {
      id: 'bar' as ProjectId,
      manifest: {
        dependencies: {
          alias: 'workspace:@scope/foo@^2.0.0',
        },
      },
      rootDir: 'bar' as ProjectRootDir,
    },
    {
      id: 'foo' as ProjectId,
      manifest: {
        name: '@scope/foo',
        version: '1.0.0',
      },
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
            alias: 'workspace:@scope/foo@^2.0.0',
          },
        },
        ['foo' as ProjectId]: {
          specifiers: {},
        },
      },
      lockfileVersion: LOCKFILE_VERSION,
    },
    workspacePackages: new Map([
      ['@scope/foo', new Map([
        ['1.0.0', {
          id: 'foo' as ProjectId,
          manifest: {
            name: '@scope/foo',
            version: '1.0.0',
          },
          rootDir: 'foo' as ProjectRootDir,
        }],
      ])],
    ]),
    lockfileDir: '',
  })).toBeFalsy()
})

test('allProjectsAreUpToDate(): works with aliased dist-tag dependency (npm:foo@latest)', async () => {
  expect(await allProjectsAreUpToDate([
    {
      id: 'bar' as ProjectId,
      manifest: {
        dependencies: {
          alias: 'npm:@scope/foo@latest',
        },
      },
      rootDir: 'bar' as ProjectRootDir,
    },
    {
      id: 'foo' as ProjectId,
      manifest: {
        name: '@scope/foo',
        version: '1.0.0',
      },
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
            alias: 'npm:@scope/foo@latest',
          },
        },
        ['foo' as ProjectId]: {
          specifiers: {},
        },
      },
      lockfileVersion: LOCKFILE_VERSION,
    },
    workspacePackages: new Map([
      ['@scope/foo', new Map([
        ['1.0.0', {
          id: 'foo' as ProjectId,
          manifest: {
            name: '@scope/foo',
            version: '1.0.0',
          },
          rootDir: 'foo' as ProjectRootDir,
        }],
      ])],
    ]),
    lockfileDir: '',
  })).toBeTruthy()
})

test('allProjectsAreUpToDate(): returns true for non-workspace range when lockfile has higher registry version and matching workspace package exists', async () => {
  expect(await allProjectsAreUpToDate([
    {
      id: 'bar' as ProjectId,
      manifest: {
        dependencies: {
          foo: '^1.0.0',
        },
      },
      rootDir: 'bar' as ProjectRootDir,
    },
    {
      id: 'foo' as ProjectId,
      manifest: {
        name: 'foo',
        version: '1.0.0',
      },
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
            foo: '1.5.0',
          },
          specifiers: {
            foo: '^1.0.0',
          },
        },
        ['foo' as ProjectId]: {
          specifiers: {},
        },
      },
      lockfileVersion: LOCKFILE_VERSION,
    },
    workspacePackages: new Map([
      ['foo', new Map([
        ['1.0.0', {
          id: 'foo' as ProjectId,
          manifest: {
            name: 'foo',
            version: '1.0.0',
          },
          rootDir: 'foo' as ProjectRootDir,
        }],
      ])],
    ]),
    lockfileDir: '',
  })).toBeTruthy()
})

test('allProjectsAreUpToDate(): returns false if injected workspace dependency version is out of date', async () => {
  expect(await allProjectsAreUpToDate([
    {
      id: 'bar' as ProjectId,
      manifest: {
        dependencies: {
          foo: 'workspace:1.0.0',
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
      manifest: {
        name: 'foo',
        version: '2.0.0',
      },
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
            foo: 'file:foo',
          },
          specifiers: {
            foo: 'workspace:1.0.0',
          },
          dependenciesMeta: {
            foo: {
              injected: true,
            },
          },
        },
        ['foo' as ProjectId]: {
          specifiers: {},
        },
      },
      packages: {
        ['foo@file:foo' as DepPath]: {
          resolution: { directory: 'foo', type: 'directory' },
        },
      },
      lockfileVersion: LOCKFILE_VERSION,
    },
    workspacePackages: new Map([
      ['foo', new Map([
        ['2.0.0', {
          id: 'foo' as ProjectId,
          manifest: {
            name: 'foo',
            version: '2.0.0',
          },
          rootDir: 'foo' as ProjectRootDir,
        }],
      ])],
    ]),
    lockfileDir: '',
  })).toBeFalsy()
})

test('allProjectsAreUpToDate(): returns true if injected workspace dependency version satisfies range', async () => {
  expect(await allProjectsAreUpToDate([
    {
      id: 'bar' as ProjectId,
      manifest: {
        dependencies: {
          foo: 'workspace:^1.0.0',
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
      manifest: {
        name: 'foo',
        version: '1.2.0',
      },
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
            foo: 'file:foo',
          },
          specifiers: {
            foo: 'workspace:^1.0.0',
          },
          dependenciesMeta: {
            foo: {
              injected: true,
            },
          },
        },
        ['foo' as ProjectId]: {
          specifiers: {},
        },
      },
      packages: {
        ['foo@file:foo' as DepPath]: {
          resolution: { directory: 'foo', type: 'directory' },
        },
      },
      lockfileVersion: LOCKFILE_VERSION,
    },
    workspacePackages: new Map([
      ['foo', new Map([
        ['1.2.0', {
          id: 'foo' as ProjectId,
          manifest: {
            name: 'foo',
            version: '1.2.0',
          },
          rootDir: 'foo' as ProjectRootDir,
        }],
      ])],
    ]),
    lockfileDir: '',
  })).toBeTruthy()
})

test('allProjectsAreUpToDate(): returns false if an aliased injected dependency version is out of date', async () => {
  expect(await allProjectsAreUpToDate([
    {
      id: 'bar' as ProjectId,
      manifest: {
        dependencies: {
          alias: 'workspace:foo@^1.0.0',
        },
        dependenciesMeta: {
          alias: {
            injected: true,
          },
        },
      },
      rootDir: 'bar' as ProjectRootDir,
    },
    {
      id: 'foo' as ProjectId,
      manifest: {
        name: 'foo',
        version: '2.0.0',
      },
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
            alias: 'foo@file:foo',
          },
          specifiers: {
            alias: 'workspace:foo@^1.0.0',
          },
          dependenciesMeta: {
            alias: {
              injected: true,
            },
          },
        },
        ['foo' as ProjectId]: {
          specifiers: {},
        },
      },
      packages: {
        ['foo@file:foo' as DepPath]: {
          resolution: { directory: 'foo', type: 'directory' },
        },
      },
      lockfileVersion: LOCKFILE_VERSION,
    },
    workspacePackages: new Map([
      ['foo', new Map([
        ['2.0.0', {
          id: 'foo' as ProjectId,
          manifest: {
            name: 'foo',
            version: '2.0.0',
          },
          rootDir: 'foo' as ProjectRootDir,
        }],
      ])],
    ]),
    lockfileDir: '',
  })).toBeFalsy()
})

test('allProjectsAreUpToDate(): returns true when an injected dependency has a peer dependency containing file:', async () => {
  expect(await allProjectsAreUpToDate([
    {
      id: 'bar' as ProjectId,
      manifest: {
        dependencies: {
          'is-positive': '^1.0.0',
        },
        dependenciesMeta: {
          'is-positive': {
            injected: true,
          },
        },
      },
      rootDir: 'bar' as ProjectRootDir,
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
            'is-positive': '1.0.0(bar@file:../bar)',
          },
          specifiers: {
            'is-positive': '^1.0.0',
          },
          dependenciesMeta: {
            'is-positive': {
              injected: true,
            },
          },
        },
      },
      lockfileVersion: LOCKFILE_VERSION,
    },
    workspacePackages: new Map(),
    lockfileDir: '',
  })).toBeTruthy()
})

test('allProjectsAreUpToDate(): returns false if a workspace dependency targets another package directory with satisfying version', async () => {
  expect(await allProjectsAreUpToDate([
    {
      id: 'bar' as ProjectId,
      manifest: {
        dependencies: {
          alias: 'workspace:foo@^1.0.0',
        },
        dependenciesMeta: {
          alias: {
            injected: true,
          },
        },
      },
      rootDir: 'bar' as ProjectRootDir,
    },
    {
      id: 'foo' as ProjectId,
      manifest: {
        name: 'foo',
        version: '1.0.0',
      },
      rootDir: 'foo' as ProjectRootDir,
    },
    {
      id: 'baz' as ProjectId,
      manifest: {
        name: 'baz',
        version: '1.0.0',
      },
      rootDir: 'baz' as ProjectRootDir,
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
            alias: 'baz@file:baz',
          },
          specifiers: {
            alias: 'workspace:foo@^1.0.0',
          },
          dependenciesMeta: {
            alias: {
              injected: true,
            },
          },
        },
        ['foo' as ProjectId]: {
          specifiers: {},
        },
        ['baz' as ProjectId]: {
          specifiers: {},
        },
      },
      packages: {
        ['baz@file:baz' as DepPath]: {
          resolution: { directory: 'baz', type: 'directory' },
        },
      },
      lockfileVersion: LOCKFILE_VERSION,
    },
    workspacePackages: new Map([
      ['foo', new Map([
        ['1.0.0', {
          id: 'foo' as ProjectId,
          manifest: {
            name: 'foo',
            version: '1.0.0',
          },
          rootDir: 'foo' as ProjectRootDir,
        }],
      ])],
      ['baz', new Map([
        ['1.0.0', {
          id: 'baz' as ProjectId,
          manifest: {
            name: 'baz',
            version: '1.0.0',
          },
          rootDir: 'baz' as ProjectRootDir,
        }],
      ])],
    ]),
    lockfileDir: '',
  })).toBeFalsy()
})

test('allProjectsAreUpToDate(): returns false if a non-injected workspace dependency has a file reference in lockfile', async () => {
  expect(await allProjectsAreUpToDate([
    {
      id: 'bar' as ProjectId,
      manifest: {
        dependencies: {
          foo: 'workspace:^1.0.0',
        },
      },
      rootDir: 'bar' as ProjectRootDir,
    },
    {
      id: 'foo' as ProjectId,
      manifest: {
        name: 'foo',
        version: '1.0.0',
      },
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
            foo: 'file:../foo',
          },
          specifiers: {
            foo: 'workspace:^1.0.0',
          },
        },
        ['foo' as ProjectId]: {
          specifiers: {},
        },
      },
      lockfileVersion: LOCKFILE_VERSION,
    },
    workspacePackages: new Map([
      ['foo', new Map([
        ['1.0.0', {
          id: 'foo' as ProjectId,
          manifest: {
            name: 'foo',
            version: '1.0.0',
          },
          rootDir: 'foo' as ProjectRootDir,
        }],
      ])],
    ]),
    lockfileDir: '',
  })).toBeFalsy()
})

test('allProjectsAreUpToDate(): returns false when link target is missing even if same-name workspace package exists at another directory', async () => {
  expect(await allProjectsAreUpToDate([
    {
      id: 'bar' as ProjectId,
      manifest: {
        dependencies: {
          foo: 'workspace:^1.0.0',
        },
      },
      rootDir: 'bar' as ProjectRootDir,
    },
    {
      id: 'other-foo' as ProjectId,
      manifest: {
        name: 'foo',
        version: '1.0.0',
      },
      rootDir: 'other-foo' as ProjectRootDir,
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
            foo: 'link:../missing-foo',
          },
          specifiers: {
            foo: 'workspace:^1.0.0',
          },
        },
        ['other-foo' as ProjectId]: {
          specifiers: {},
        },
      },
      lockfileVersion: LOCKFILE_VERSION,
    },
    workspacePackages: new Map([
      ['foo', new Map([
        ['1.0.0', {
          id: 'other-foo' as ProjectId,
          manifest: {
            name: 'foo',
            version: '1.0.0',
          },
          rootDir: 'other-foo' as ProjectRootDir,
        }],
      ])],
    ]),
    lockfileDir: '',
  })).toBeFalsy()
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
        'is-positive@2.0.0': {
          resolution: { integrity: 'sha512-AAAA' },
        },
      },
      lockfileVersion: LOCKFILE_VERSION,
    } as LockfileObject,
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

  test('allProjectsAreUpToDate(): returns false if a dependency named like an Object.prototype property is added to local file', async () => {
    await writeFile('./local-dir/package.json', JSON.stringify({
      name: 'local-dir',
      version: '1.0.0',
      dependencies: {
        'is-positive': '2.0.0',
        constructor: '1.0.0',
      },
    }))
    expect(await allProjectsAreUpToDate(projects, {
      ...options,
      lockfileDir: process.cwd(),
    })).toBeFalsy()
  })

  test('allProjectsAreUpToDate(): returns false if a dependency named like an Object.prototype property is removed from local file', async () => {
    const localDirSnapshot = options.wantedLockfile.packages!['local@file:./local-dir' as DepPath]
    expect(await allProjectsAreUpToDate(projects, {
      ...options,
      wantedLockfile: {
        ...options.wantedLockfile,
        packages: {
          'local@file:./local-dir': {
            ...localDirSnapshot,
            dependencies: {
              ...localDirSnapshot.dependencies,
              constructor: '1.0.0',
            },
          },
        } as LockfileObject['packages'],
      },
      lockfileDir: process.cwd(),
    })).toBeFalsy()
  })

  test('allProjectsAreUpToDate(): returns true if the locked version satisfies a range in local file', async () => {
    await writeLocalDirManifest({ dependencies: { 'is-positive': '^2.0.0' } })
    expect(await allProjectsAreUpToDate(projects, { ...options, lockfileDir: process.cwd() })).toBeTruthy()
  })

  test('allProjectsAreUpToDate(): reads the dependencies of local file after the readPackage hook', async () => {
    await writeLocalDirManifest({ dependencies: { 'is-positive': '3.0.0' } })
    expect(await allProjectsAreUpToDate(projects, {
      ...options,
      lockfileDir: process.cwd(),
      readPackageHook: (manifest) => ({ ...manifest, dependencies: { 'is-positive': '2.0.0' } }),
    })).toBeTruthy()
  })

  test('allProjectsAreUpToDate(): resolves a catalog dependency of local file from the catalogs', async () => {
    await writeLocalDirManifest({ dependencies: { 'is-positive': 'catalog:' } })
    const wantedLockfile = { ...options.wantedLockfile, catalogs: { default: { 'is-positive': { specifier: '2.0.0', version: '2.0.0' } } } }
    expect(await allProjectsAreUpToDate(projects, {
      ...options,
      lockfileDir: process.cwd(),
      catalogs: { default: { 'is-positive': '2.0.0' } },
      wantedLockfile,
    })).toBeTruthy()
    expect(await allProjectsAreUpToDate(projects, {
      ...options,
      lockfileDir: process.cwd(),
      catalogs: { default: { 'is-positive': '3.0.0' } },
      wantedLockfile: { ...options.wantedLockfile, catalogs: { default: { 'is-positive': { specifier: '3.0.0', version: '3.0.0' } } } },
    })).toBeFalsy()
  })

  test('checkLinkedPackagesAreUpToDate(): reports a dependency of local file that became optional on a frozen install', async () => {
    await writeLocalDirManifest({ optionalDependencies: { 'is-positive': '2.0.0' } })
    expect((await checkLinkedPackagesAreUpToDate({
      linkWorkspacePackages: true,
      manifestsByDir: {},
      lockfilePackages: options.wantedLockfile.packages,
      lockfileDir: process.cwd(),
      resolution: { frozenLockfile: true },
    }, {
      dir: path.resolve('bar'),
      manifest: projects[0].manifest,
      snapshot: options.wantedLockfile.importers['bar' as ProjectId],
    })).upToDate).toBe(false)
  })

  test('checkLinkedPackagesAreUpToDate(): accepts an unresolved optional dependency of local file only on a frozen install', async () => {
    await writeLocalDirManifest({ dependencies: { 'is-positive': '2.0.0' }, optionalDependencies: { 'is-odd': '1.0.0' } })
    const check = async (frozenLockfile: boolean) => (await checkLinkedPackagesAreUpToDate({
      linkWorkspacePackages: true,
      manifestsByDir: {},
      lockfilePackages: options.wantedLockfile.packages,
      lockfileDir: process.cwd(),
      resolution: { frozenLockfile },
    }, {
      dir: path.resolve('bar'),
      manifest: projects[0].manifest,
      snapshot: options.wantedLockfile.importers['bar' as ProjectId],
    })).upToDate
    expect(await check(true)).toBe(true)
    expect(await check(false)).toBe(false)
  })

  const commit = '97edff6f525f192a3f83cea1944765f769ae2678'
  const otherCommit = '1'.repeat(40)
  const codeload = `https://codeload.github.com/kevva/is-positive/tar.gz/${commit}`
  test.each([
    ['a tarball', 'https://example.com/is-positive-2.0.0.tgz', 'https://example.com/is-positive-3.0.0.tgz', 'https://example.com/is-positive-2.0.0.tgz'],
    ['a git commit', `github:kevva/is-positive#${commit}`, `github:kevva/is-positive#${otherCommit}`, codeload],
    ['an https git commit', `https://github.com/kevva/is-positive.git#${commit}`, `https://github.com/kevva/is-positive.git#${otherCommit}`, codeload],
    ['a git commit with a path', `github:kevva/is-positive#${commit}&path:/sub`, `github:kevva/is-positive#${otherCommit}&path:/sub`, `${codeload}#path:/sub`],
    ['a tarball that another URL ends with', 'https://good.test/a.tgz', 'https://good.test/a.tgz', 'https://evil.test/x@https://good.test/a.tgz', false],
    ['a commit that names the repository', `github:kevva/${commit}#${otherCommit}`, `github:kevva/${commit}#${commit}`, `https://codeload.github.com/kevva/${commit}/tar.gz/${otherCommit}`],
    ['a tarball whose URL ends in parentheses', 'https://example.com/pkg(a).tgz', 'https://example.com/pkg(b).tgz', 'https://example.com/pkg(a).tgz'],
    ['a gitlab commit', `gitlab:kevva/is-positive#${commit}`, `gitlab:kevva/is-positive#${otherCommit}`, `https://gitlab.com/kevva/is-positive/-/archive/${commit}/is-positive-${commit}.tar.gz`],
    ['a bitbucket commit', `bitbucket:kevva/is-positive#${commit}`, `bitbucket:kevva/is-positive#${otherCommit}`, `https://bitbucket.org/kevva/is-positive/get/${commit}.tar.gz`],
    ['a git path', `github:kevva/is-positive#${commit}&path:/sub`, `github:kevva/is-positive#${commit}&path:/other`, `${codeload}#path:/sub`],
  ])('checkLinkedPackagesAreUpToDate(): accepts %s dependency of local file only on a frozen install that keeps its source', async (_, lockedSpec, otherSpec, lockedRef, keepsLockedSpec = true) => {
    const lockfilePackages = {
      ...options.wantedLockfile.packages,
      'local@file:./local-dir': {
        ...options.wantedLockfile.packages!['local@file:./local-dir' as DepPath],
        dependencies: { 'is-positive': lockedRef },
      },
      [`is-positive@${lockedRef}`]: { resolution: { tarball: lockedRef } },
    } as LockfileObject['packages']
    const check = async (spec: string, frozenLockfile: boolean) => {
      await writeLocalDirManifest({ dependencies: { 'is-positive': spec } })
      return (await checkLinkedPackagesAreUpToDate({
        linkWorkspacePackages: true,
        manifestsByDir: {},
        lockfilePackages,
        lockfileDir: process.cwd(),
        resolution: { frozenLockfile },
      }, {
        dir: path.resolve('bar'),
        manifest: projects[0].manifest,
        snapshot: options.wantedLockfile.importers['bar' as ProjectId],
      })).upToDate
    }
    expect(await check(lockedSpec, true)).toBe(keepsLockedSpec)
    expect(await check(lockedSpec, false)).toBe(false)
    expect(await check(otherSpec, true)).toBe(false)
  })

  test('allProjectsAreUpToDate(): accepts a peer of local file that is also its dependency whether or not the lockfile records it', async () => {
    await writeLocalDirManifest({
      dependencies: { 'is-positive': '2.0.0' },
      peerDependencies: { 'is-positive': '^2.0.0' },
    })
    const localDirSnapshot = options.wantedLockfile.packages!['local@file:./local-dir' as DepPath]
    const check = async (snapshot: PackageSnapshot) => allProjectsAreUpToDate(projects, {
      ...options,
      lockfileDir: process.cwd(),
      wantedLockfile: {
        ...options.wantedLockfile,
        packages: { ...options.wantedLockfile.packages, 'local@file:./local-dir': snapshot } as LockfileObject['packages'],
      },
    })
    expect(await check({ ...localDirSnapshot, peerDependencies: { 'is-positive': '^2.0.0' } })).toBeTruthy()
    expect(await check(localDirSnapshot)).toBeTruthy()
    expect(await check({ ...localDirSnapshot, peerDependencies: { 'is-positive': '^3.0.0' } })).toBeFalsy()
  })

  test('allProjectsAreUpToDate(): reads the dependencies of local file from a package.yaml', async () => {
    await rm('./local-dir/package.json')
    await writeFile('./local-dir/package.yaml', 'name: local-dir\nversion: 1.0.0\ndependencies:\n  is-positive: 2.0.0\n')
    expect(await allProjectsAreUpToDate(projects, { ...options, lockfileDir: process.cwd() })).toBeTruthy()
    await writeFile('./local-dir/package.yaml', 'name: local-dir\nversion: 1.0.0\ndependencies:\n  is-positive: 2.0.0\n  is-negative: 1.0.0\n')
    expect(await allProjectsAreUpToDate(projects, { ...options, lockfileDir: process.cwd() })).toBeFalsy()
  })

  test('allProjectsAreUpToDate(): keeps the manifest of local file when the readPackage hook returns nothing', async () => {
    expect(await allProjectsAreUpToDate(projects, {
      ...options,
      lockfileDir: process.cwd(),
      readPackageHook: (() => undefined) as unknown as ReadPackageHook,
    })).toBeTruthy()
  })

  test('checkLinkedPackagesAreUpToDate(): accepts an unchanged SSH git dependency of local file on a frozen install', async () => {
    const gitRef = 'git+ssh://git@github.com/org/repo.git#0000000000000000000000000000000000000000'
    await writeLocalDirManifest({ dependencies: { repo: 'git+ssh://git@github.com/org/repo.git' } })
    const localDirSnapshot = options.wantedLockfile.packages!['local@file:./local-dir' as DepPath]
    expect((await checkLinkedPackagesAreUpToDate({
      linkWorkspacePackages: true,
      manifestsByDir: {},
      lockfilePackages: {
        'local@file:./local-dir': { ...localDirSnapshot, dependencies: { repo: gitRef } },
        [`repo@${gitRef}`]: {
          resolution: { type: 'git', repo: 'ssh://git@github.com/org/repo.git', commit: '0000000000000000000000000000000000000000' },
        },
      } as LockfileObject['packages'],
      lockfileDir: process.cwd(),
      resolution: { frozenLockfile: true },
    }, {
      dir: path.resolve('bar'),
      manifest: projects[0].manifest,
      snapshot: options.wantedLockfile.importers['bar' as ProjectId],
    })).upToDate).toBe(true)
  })

  test('checkLinkedPackagesAreUpToDate(): rereads the manifest of local file when a context is reused', async () => {
    const ctx = {
      linkWorkspacePackages: true,
      manifestsByDir: {},
      lockfilePackages: options.wantedLockfile.packages,
      lockfileDir: process.cwd(),
    }
    const project = {
      dir: path.resolve('bar'),
      manifest: projects[0].manifest,
      snapshot: options.wantedLockfile.importers['bar' as ProjectId],
    }
    expect((await checkLinkedPackagesAreUpToDate(ctx, project)).upToDate).toBe(true)
    await writeLocalDirManifest({ dependencies: { 'is-positive': '3.0.0' } })
    expect((await checkLinkedPackagesAreUpToDate(ctx, project)).upToDate).toBe(false)
  })

  test('allProjectsAreUpToDate(): accepts an aliased file dependency of local file', async () => {
    await mkdir('other-dir')
    await writeFile('./other-dir/package.json', JSON.stringify({ name: 'other', version: '1.0.0' }))
    await writeLocalDirManifest({ dependencies: { renamed: 'file:../other-dir' } })
    const localDirSnapshot = options.wantedLockfile.packages!['local@file:./local-dir' as DepPath]
    const check = async (ref: string) => allProjectsAreUpToDate(projects, {
      ...options,
      lockfileDir: process.cwd(),
      wantedLockfile: {
        ...options.wantedLockfile,
        packages: {
          'local@file:./local-dir': { ...localDirSnapshot, dependencies: { renamed: ref } },
          'other@file:other-dir': { resolution: { directory: 'other-dir', type: 'directory' } },
        } as LockfileObject['packages'],
      },
    })
    expect(await check('other@file:other-dir')).toBeTruthy()
    expect(await check('other@file:local-dir')).toBeFalsy()
  })

  async function writeLocalDirManifest (fields: Partial<DependencyManifest>): Promise<void> {
    await writeFile('./local-dir/package.json', JSON.stringify({ name: 'local-dir', version: '1.0.0', ...fields }))
  }
})

describe('transitive local directory dependency', () => {
  beforeEach(async () => {
    prepareEmpty()
    await mkdir('a')
    await mkdir('b')
    await mkdir('leaf1')
    await mkdir('leaf2')
    await writeFile('./a/package.json', JSON.stringify({ name: 'a', version: '1.0.0', dependencies: { b: 'file:../b' } }))
    await writeFile('./b/package.json', JSON.stringify({ name: 'b', version: '1.0.0', dependencies: { leaf: 'file:../leaf1' } }))
    await writeFile('./leaf1/package.json', JSON.stringify({ name: 'leaf', version: '1.0.0' }))
    await writeFile('./leaf2/package.json', JSON.stringify({ name: 'leaf', version: '2.0.0' }))
  })
  const lockfilePackages = {
    'a@file:./a': {
      resolution: { directory: './a', type: 'directory' },
      version: '1.0.0',
      dependencies: { b: 'file:./b' },
    },
    'b@file:./b': {
      resolution: { directory: './b', type: 'directory' },
      version: '1.0.0',
      dependencies: { leaf: 'file:./leaf1' },
    },
    'leaf@file:./leaf1': {
      resolution: { directory: './leaf1', type: 'directory' },
      version: '1.0.0',
    },
  } as LockfileObject['packages']
  const project = {
    dir: path.resolve('bar'),
    manifest: { dependencies: { a: 'file:./a' } },
    snapshot: {
      dependencies: { a: 'file:./a' },
      specifiers: { a: 'file:./a' },
    } as ProjectSnapshot,
  }
  const ctx = () => ({
    linkWorkspacePackages: true,
    manifestsByDir: {},
    lockfilePackages,
    lockfileDir: process.cwd(),
  })
  test('checkLinkedPackagesAreUpToDate(): accepts an unchanged chain of local directory dependencies', async () => {
    expect((await checkLinkedPackagesAreUpToDate(ctx(), project)).upToDate).toBe(true)
  })
  test('checkLinkedPackagesAreUpToDate(): reports a transitive local directory dependency whose own dependencies changed (pnpm/pnpm#17128)', async () => {
    await writeFile('./b/package.json', JSON.stringify({ name: 'b', version: '1.0.0', dependencies: { leaf: 'file:../leaf2' } }))
    expect((await checkLinkedPackagesAreUpToDate(ctx(), project)).upToDate).toBe(false)
  })
  test('checkLinkedPackagesAreUpToDate(): reports a local directory child whose lockfile snapshot is missing', async () => {
    const packagesWithoutB = { ...lockfilePackages } as Record<string, unknown>
    delete packagesWithoutB['b@file:./b']
    expect((await checkLinkedPackagesAreUpToDate({ ...ctx(), lockfilePackages: packagesWithoutB as LockfileObject['packages'] }, project)).upToDate).toBe(false)
  })
  test('checkLinkedPackagesAreUpToDate(): terminates on a cycle of local directory dependencies', async () => {
    await writeFile('./b/package.json', JSON.stringify({ name: 'b', version: '1.0.0', dependencies: { a: 'file:../a' } }))
    const cyclicPackages = {
      ...lockfilePackages,
      'b@file:./b': {
        resolution: { directory: './b', type: 'directory' },
        version: '1.0.0',
        dependencies: { a: 'file:./a' },
      },
    }
    expect((await checkLinkedPackagesAreUpToDate({ ...ctx(), lockfilePackages: cyclicPackages }, project)).upToDate).toBe(true)
  })
  test('checkLinkedPackagesAreUpToDate(): accepts a nested directory whose peer an ancestor supplies', async () => {
    await mkdir('c')
    await writeFile('./a/package.json', JSON.stringify({ name: 'a', version: '1.0.0', dependencies: { b: 'file:../b', p: '2.0.0' } }))
    await writeFile('./b/package.json', JSON.stringify({ name: 'b', version: '1.0.0', dependencies: { c: 'file:../c' } }))
    await writeFile('./c/package.json', JSON.stringify({ name: 'c', version: '1.0.0', dependencies: { p: '1.0.0' }, peerDependencies: { p: '*' } }))
    const packages = {
      'a@file:./a': {
        resolution: { directory: './a', type: 'directory' },
        version: '1.0.0',
        dependencies: { b: 'file:./b', p: '2.0.0' },
      },
      'b@file:./b': {
        resolution: { directory: './b', type: 'directory' },
        version: '1.0.0',
        dependencies: { c: 'file:./c(p@2.0.0)' },
      },
      'c@file:./c(p@2.0.0)': {
        resolution: { directory: './c', type: 'directory' },
        version: '1.0.0',
        dependencies: { p: '2.0.0' },
        peerDependencies: { p: '*' },
      },
      'p@2.0.0': {
        resolution: { integrity: 'sha512-p' },
        version: '2.0.0',
      },
    } as LockfileObject['packages']
    expect((await checkLinkedPackagesAreUpToDate({ ...ctx(), lockfilePackages: packages, resolution: { autoInstallPeers: false } }, project)).upToDate).toBe(true)
  })
  test('checkLinkedPackagesAreUpToDate(): checks a shared directory again where no ancestor supplies its peer', async () => {
    await mkdir('c')
    await mkdir('e')
    await mkdir('f')
    await writeFile('./a/package.json', JSON.stringify({ name: 'a', version: '1.0.0', dependencies: { b: 'file:../b', e: 'file:../e' } }))
    await writeFile('./b/package.json', JSON.stringify({ name: 'b', version: '1.0.0', dependencies: { c: 'file:../c', p: '2.0.0' } }))
    await writeFile('./c/package.json', JSON.stringify({ name: 'c', version: '1.0.0', dependencies: { p: '1.0.0' }, peerDependencies: { p: '*' } }))
    await writeFile('./e/package.json', JSON.stringify({ name: 'e', version: '1.0.0', dependencies: { f: 'file:../f' } }))
    await writeFile('./f/package.json', JSON.stringify({ name: 'f', version: '1.0.0', dependencies: { c: 'file:../c' } }))
    const directory = (dir: string, dependencies: Record<string, string>) => ({
      resolution: { directory: dir, type: 'directory' },
      version: '1.0.0',
      dependencies,
    })
    const packages = {
      'a@file:./a': directory('./a', { b: 'file:./b', e: 'file:./e' }),
      'b@file:./b': directory('./b', { c: 'file:./c(p@2.0.0)', p: '2.0.0' }),
      'c@file:./c(p@2.0.0)': { ...directory('./c', { p: '2.0.0' }), peerDependencies: { p: '*' } },
      'e@file:./e': directory('./e', { f: 'file:./f' }),
      'f@file:./f': directory('./f', { c: 'file:./c(p@2.0.0)' }),
      'p@2.0.0': { resolution: { integrity: 'sha512-p' }, version: '2.0.0' },
    } as LockfileObject['packages']
    expect((await checkLinkedPackagesAreUpToDate({ ...ctx(), lockfilePackages: packages, resolution: { autoInstallPeers: false } }, project)).upToDate).toBe(false)
  })

  test('checkLinkedPackagesAreUpToDate(): checks a peer-free shared directory again where a descendant peer is not supplied', async () => {
    await mkdir('c')
    await mkdir('d')
    await mkdir('e')
    await mkdir('g')
    await writeFile('./a/package.json', JSON.stringify({ name: 'a', version: '1.0.0', dependencies: { b: 'file:../b', e: 'file:../e' } }))
    await writeFile('./b/package.json', JSON.stringify({ name: 'b', version: '1.0.0', dependencies: { c: 'file:../c', p: '2.0.0' } }))
    await writeFile('./c/package.json', JSON.stringify({ name: 'c', version: '1.0.0', dependencies: { d: 'file:../d' } }))
    await writeFile('./d/package.json', JSON.stringify({ name: 'd', version: '1.0.0', dependencies: { p: '1.0.0' }, peerDependencies: { p: '*' } }))
    await writeFile('./e/package.json', JSON.stringify({ name: 'e', version: '1.0.0', dependencies: { g: 'file:../g' } }))
    await writeFile('./g/package.json', JSON.stringify({ name: 'g', version: '1.0.0', dependencies: { c: 'file:../c' } }))
    const directory = (dir: string, dependencies: Record<string, string>) => ({
      resolution: { directory: dir, type: 'directory' },
      version: '1.0.0',
      dependencies,
    })
    const packages = {
      'a@file:./a': directory('./a', { b: 'file:./b', e: 'file:./e' }),
      'b@file:./b': directory('./b', { c: 'file:./c', p: '2.0.0' }),
      'c@file:./c': directory('./c', { d: 'file:./d' }),
      'd@file:./d': { ...directory('./d', { p: '2.0.0' }), peerDependencies: { p: '*' } },
      'e@file:./e': directory('./e', { g: 'file:./g' }),
      'g@file:./g': directory('./g', { c: 'file:./c' }),
      'p@2.0.0': { resolution: { integrity: 'sha512-p' }, version: '2.0.0' },
    } as LockfileObject['packages']
    // Through b, p is supplied, so d's own p is shadowed. Through e it is not,
    // so d keeps p@1.0.0 and the recorded 2.0.0 is stale.
    expect((await checkLinkedPackagesAreUpToDate({ ...ctx(), lockfilePackages: packages, resolution: { autoInstallPeers: false } }, project)).upToDate).toBe(false)
  })
})

describe('local tgz file dependency', () => {
  beforeEach(async () => {
    prepareEmpty()
  })

  const projects = [
    {
      id: 'bar' as ProjectId,
      manifest: {
        dependencies: {
          'local-tarball': 'file:local-tarball.tar',
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

  const wantedLockfile: LockfileObject = {
    lockfileVersion: LOCKFILE_VERSION,
    importers: {
      ['bar' as ProjectId]: {
        dependencies: { 'local-tarball': 'file:local-tarball.tar' },
        specifiers: { 'local-tarball': 'file:local-tarball.tar' },
      },
      ['foo' as ProjectId]: {
        specifiers: {},
      },
    },
    packages: {
      ['local-tarball@file:local-tarball.tar' as DepPath]: {
        resolution: {
          integrity: 'sha512-nQP7gWOhNQ/5HoM/rJmzOgzZt6Wg6k56CyvO/0sMmiS3UkLSmzY5mW8mMrnbspgqpmOW8q/FHyb0YIr4n2A8VQ==',
          tarball: 'file:local-tarball.tar',
        },
        version: '1.0.0',
      },
    },
  }

  const options = {
    autoInstallPeers: false,
    catalogs: {},
    excludeLinksFromLockfile: false,
    linkWorkspacePackages: true,
    wantedLockfile,
    workspacePackages,
    lockfileDir: process.cwd(),
  }

  test('allProjectsAreUpToDate(): returns true if local file not changed', async () => {
    expect.hasAssertions()

    const pack = tar.pack()
    pack.entry({ name: 'package.json', mtime: new Date('1970-01-01T00:00:00.000Z') }, JSON.stringify({
      name: 'local-tarball',
      version: '1.0.0',
    }))
    pack.finalize()

    await pipeline(pack, createWriteStream('./local-tarball.tar'))

    // Make the test is set up correctly and the local-tarball.tar created above
    // has the expected integrity hash.
    await expect(getTarballIntegrity('./local-tarball.tar')).resolves.toBe('sha512-nQP7gWOhNQ/5HoM/rJmzOgzZt6Wg6k56CyvO/0sMmiS3UkLSmzY5mW8mMrnbspgqpmOW8q/FHyb0YIr4n2A8VQ==')

    const lockfileDir = process.cwd()
    expect(await allProjectsAreUpToDate(projects, { ...options, lockfileDir })).toBeTruthy()
  })

  test('allProjectsAreUpToDate(): returns false if local file has changed', async () => {
    expect.hasAssertions()

    const pack = tar.pack()
    pack.entry({ name: 'package.json', mtime: new Date('2000-01-01T00:00:00') }, JSON.stringify({
      name: 'local-tarball',
      version: '1.0.0',
    }))
    pack.entry({ name: 'newly-added-file.txt' }, 'This file changes the tarball.')
    pack.finalize()
    await pipeline(pack, createWriteStream('./local-tarball.tar'))

    const lockfileDir = process.cwd()
    expect(await allProjectsAreUpToDate(projects, { ...options, lockfileDir })).toBeFalsy()
  })

  test('findPackageTarballIntegrityMismatch(): reports a changed local file', async () => {
    expect.hasAssertions()

    const pack = tar.pack()
    pack.entry({ name: 'package.json', mtime: new Date('2000-01-01T00:00:00') }, JSON.stringify({
      name: 'local-tarball',
      version: '1.0.0',
    }))
    pack.entry({ name: 'newly-added-file.txt' }, 'This file changes the tarball.')
    pack.finalize()
    await pipeline(pack, createWriteStream('./local-tarball.tar'))

    const lockfileDir = process.cwd()
    const expected = (wantedLockfile.packages!['local-tarball@file:local-tarball.tar' as DepPath]!.resolution as TarballResolution).integrity
    await expect(findPackageTarballIntegrityMismatch({
      fileIntegrityCache: new Map(),
      lockfileDir,
    }, wantedLockfile.packages!['local-tarball@file:local-tarball.tar' as DepPath]!)).resolves.toMatchObject({
      expected,
      path: path.join(lockfileDir, 'local-tarball.tar'),
    })
  })

  test('allProjectsAreUpToDate(): returns false if local dep does not exist', async () => {
    expect.hasAssertions()

    const lockfileDir = process.cwd()
    expect(await allProjectsAreUpToDate(projects, { ...options, lockfileDir })).toBeFalsy()
  })

  test('findPackageTarballIntegrityMismatch(): rejects unreadable files with their path', async () => {
    const lockfileDir = process.cwd()
    await expect(findPackageTarballIntegrityMismatch({
      fileIntegrityCache: new Map(),
      lockfileDir,
    }, wantedLockfile.packages!['local-tarball@file:local-tarball.tar' as DepPath]!)).rejects.toThrow(path.join(lockfileDir, 'local-tarball.tar'))
  })

  test('findPackageTarballIntegrityMismatch(): reuses the file read across snapshots', async () => {
    const lockfileDir = process.cwd()
    await writeFile('local-tarball.tar', 'first')
    const ctx = { fileIntegrityCache: new Map<string, Promise<string>>(), lockfileDir }
    const snapshot = wantedLockfile.packages!['local-tarball@file:local-tarball.tar' as DepPath]!
    const first = await findPackageTarballIntegrityMismatch(ctx, snapshot)
    await writeFile('local-tarball.tar', 'second')
    expect(await findPackageTarballIntegrityMismatch(ctx, snapshot)).toEqual(first)
    expect(ctx.fileIntegrityCache.size).toBe(1)
  })

  test('findPackageTarballIntegrityMismatch(): recovers tarball path from depPath when resolution.tarball is omitted', async () => {
    const lockfileDir = process.cwd()
    await writeFile('local-tarball.tar', 'content')
    const ctx = { fileIntegrityCache: new Map<string, Promise<string>>(), lockfileDir }
    const snapshot: PackageSnapshot = {
      resolution: {
        integrity: 'sha512-expected',
      },
    }
    const mismatch = await findPackageTarballIntegrityMismatch(
      ctx,
      snapshot,
      'local-tarball@file:local-tarball.tar'
    )
    expect(mismatch).toMatchObject({
      expected: 'sha512-expected',
      path: path.join(lockfileDir, 'local-tarball.tar'),
    })
  })

  test('findPackageTarballIntegrityMismatch(): accepts valid non-sha512 SRI (e.g. sha1) and detects changes', async () => {
    const lockfileDir = process.cwd()
    await writeFile('local-tarball.tar', 'hello world')
    const ctx = { fileIntegrityCache: new Map<string, Promise<string>>(), lockfileDir }
    const sha1Expected = 'sha1-Kq5sNclPz7QV2+lfQIuc6R7oRu0='
    const snapshot: PackageSnapshot = {
      resolution: {
        integrity: sha1Expected,
        tarball: 'file:local-tarball.tar',
      },
    }
    await expect(findPackageTarballIntegrityMismatch(ctx, snapshot)).resolves.toBeNull()

    const changedSnapshot: PackageSnapshot = {
      resolution: {
        integrity: 'sha1-AAAAAAAAAAAAAAAAAAAAAAAAAAA=',
        tarball: 'file:local-tarball.tar',
      },
    }
    const mismatch = await findPackageTarballIntegrityMismatch(ctx, changedSnapshot)
    expect(mismatch).toMatchObject({
      expected: 'sha1-AAAAAAAAAAAAAAAAAAAAAAAAAAA=',
      found: sha1Expected,
      path: path.join(lockfileDir, 'local-tarball.tar'),
    })
  })

  test('resolveLocalTarballPath(): rejects UNC and invalid paths, resolves relative paths', () => {
    const lockfileDir = '/workspace/root'
    expect(resolveLocalTarballPath(lockfileDir, 'file://server/share/pkg.tgz')).toBeUndefined()
    expect(resolveLocalTarballPath(lockfileDir, 'file:\\\\server\\share\\pkg.tgz')).toBeUndefined()
    expect(resolveLocalTarballPath(lockfileDir, 'file:////server/share/pkg.tgz')).toBeUndefined()
    expect(resolveLocalTarballPath(lockfileDir, 'file:pkg\0.tgz')).toBeUndefined()
    expect(resolveLocalTarballPath(lockfileDir, 'not-a-file-protocol')).toBeUndefined()
    expect(resolveLocalTarballPath(lockfileDir, 'file:/etc/passwd')).toBeUndefined()
    expect(resolveLocalTarballPath(lockfileDir, 'file:../../etc/shadow')).toBeUndefined()
    expect(resolveLocalTarballPath(lockfileDir, 'file:./vendor/tar.tgz')).toBe(path.resolve(lockfileDir, './vendor/tar.tgz'))
    expect(resolveLocalTarballPath(lockfileDir, 'file:vendor/tar.tgz')).toBe(path.resolve(lockfileDir, 'vendor/tar.tgz'))
    expect(resolveLocalTarballPath(lockfileDir, 'file:./vendor/tar.tar.gz')).toBe(path.resolve(lockfileDir, './vendor/tar.tar.gz'))
    expect(resolveLocalTarballPath(lockfileDir, 'file:./vendor/tar.tar')).toBe(path.resolve(lockfileDir, './vendor/tar.tar'))
    expect(resolveLocalTarballPath(lockfileDir, 'file:./vendor/tar.tar.bz2')).toBe(path.resolve(lockfileDir, './vendor/tar.tar.bz2'))
    expect(resolveLocalTarballPath(lockfileDir, 'file:./vendor/tar.tbz2')).toBe(path.resolve(lockfileDir, './vendor/tar.tbz2'))
    expect(resolveLocalTarballPath(lockfileDir, 'file:./vendor/tar.tbz')).toBe(path.resolve(lockfileDir, './vendor/tar.tbz'))
  })

  test.each(['tar.bz2', 'tbz2', 'tbz'])('findPackageTarballIntegrityMismatch(): detects changed bzip archive (%s)', async (ext) => {
    const lockfileDir = process.cwd()
    const fileName = `local-tarball.${ext}`
    await writeFile(fileName, 'archive content')
    const ctx = { fileIntegrityCache: new Map<string, Promise<string>>(), lockfileDir }
    const snapshot: PackageSnapshot = {
      resolution: {
        integrity: 'sha512-expectedMismatch',
        tarball: `file:${fileName}`,
      },
    }
    const mismatch = await findPackageTarballIntegrityMismatch(ctx, snapshot)
    expect(mismatch).toMatchObject({
      expected: 'sha512-expectedMismatch',
      path: path.join(lockfileDir, fileName),
    })
  })

  test('findPackageTarballIntegrityMismatch(): returns null on malformed lockfile entries', async () => {
    const lockfileDir = process.cwd()
    const ctx = { fileIntegrityCache: new Map<string, Promise<string>>(), lockfileDir }
    expect(await findPackageTarballIntegrityMismatch(ctx, undefined)).toBeNull()
    expect(await findPackageTarballIntegrityMismatch(ctx, {} as PackageSnapshot)).toBeNull()
    expect(await findPackageTarballIntegrityMismatch(ctx, { resolution: {} } as PackageSnapshot)).toBeNull()
    expect(await findPackageTarballIntegrityMismatch(ctx, { resolution: { integrity: '' } } as PackageSnapshot)).toBeNull()
    expect(await findPackageTarballIntegrityMismatch(ctx, { resolution: { integrity: 'sha512-abc', tarball: 'file://unc/share.tgz' } } as PackageSnapshot)).toBeNull()
    expect(await findPackageTarballIntegrityMismatch(ctx, { resolution: { integrity: 'sha512-abc' } } as PackageSnapshot, 'invalid-dep-path-no-at')).toBeNull()
  })
})

// Regression tests for https://github.com/pnpm/pnpm/pull/9807.
describe('local tgz file dependency with peer dependencies', () => {
  beforeEach(async () => {
    prepareEmpty()
  })

  const projects = [
    {
      id: 'bar' as ProjectId,
      manifest: {
        dependencies: {
          '@pnpm.e2e/foo': '1.0.0',
          'local-tarball': 'file:local-tarball.tar',
        },
      },
      rootDir: 'bar' as ProjectRootDir,
    },
  ]

  const wantedLockfile: LockfileObject = {
    lockfileVersion: LOCKFILE_VERSION,
    importers: {
      ['bar' as ProjectId]: {
        dependencies: {
          '@pnpm.e2e/foo': '1.0.0',
          'local-tarball': 'file:local-tarball.tar(@pnpm.e2e/foo@1.0.0)',
        },
        specifiers: {
          '@pnpm.e2e/foo': '1.0.0',
          'local-tarball': 'file:local-tarball.tar',
        },
      },
    },
    packages: {
      ['@pnpm.e2e/foo@1.0.0' as DepPath]: {
        resolution: {
          integrity: 'sha512-/HITDx7DEbvGeznQ5aq9qK5rn7YlVGST+fW2cQ0QAoO7/kVn/QJkN7VYAB0nvRIFkFsaAMJZ61zB8pJo9Fonng==',
        },
        version: '1.0.0',
      },
      ['local-tarball@file:local-tarball.tar(@pnpm.e2e/foo@1.0.0)' as DepPath]: {
        resolution: {
          integrity: 'sha512-dVXphRGPXHhIt6CKeest8Tkbva4FatStRw4PZbJ4zFszWppqAkZureR6mOF0mT/9Drr5wZ5y9tPaqcmsf/a5cw==',
          tarball: 'file:local-tarball.tar',
        },
        version: '1.0.0',
        dependencies: {
          '@pnpm.e2e/foo': '1.0.0',
        },
      },
    },
  }

  const options = {
    autoInstallPeers: false,
    catalogs: {},
    excludeLinksFromLockfile: false,
    linkWorkspacePackages: true,
    wantedLockfile,
    workspacePackages,
    lockfileDir: process.cwd(),
  }

  test('allProjectsAreUpToDate(): returns true if local file not changed', async () => {
    expect.hasAssertions()

    const pack = tar.pack()
    pack.entry({ name: 'package.json', mtime: new Date('1970-01-01T00:00:00.000Z') }, JSON.stringify({
      name: 'local-tarball',
      version: '1.0.0',
      peerDependencies: {
        '@pnpm.e2e/foo': '1.0.0',
      },
    }))
    pack.finalize()

    await pipeline(pack, createWriteStream('./local-tarball.tar'))

    // Make sure the test is set up correctly and the local-tarball.tar created
    // above has the expected integrity hash.
    await expect(getTarballIntegrity('./local-tarball.tar')).resolves.toBe('sha512-dVXphRGPXHhIt6CKeest8Tkbva4FatStRw4PZbJ4zFszWppqAkZureR6mOF0mT/9Drr5wZ5y9tPaqcmsf/a5cw==')

    const lockfileDir = process.cwd()
    expect(await allProjectsAreUpToDate(projects, { ...options, lockfileDir })).toBeTruthy()
  })

  test('allProjectsAreUpToDate(): returns false if local file has changed', async () => {
    expect.hasAssertions()

    const pack = tar.pack()
    pack.entry({ name: 'package.json', mtime: new Date('2000-01-01T00:00:00') }, JSON.stringify({
      name: 'local-tarball',
      // Incrementing the version from 1.0.0 to 2.0.0.
      version: '2.0.0',
      peerDependencies: {
        '@pnpm.e2e/foo': '1.0.0',
      },
    }))
    pack.finalize()
    await pipeline(pack, createWriteStream('./local-tarball.tar'))

    const lockfileDir = process.cwd()
    expect(await allProjectsAreUpToDate(projects, { ...options, lockfileDir })).toBeFalsy()
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
    } as LockfileObject,
    workspacePackages,
    lockfileDir: process.cwd(),
  }
  expect(await allProjectsAreUpToDate(projects, {
    ...options,
    lockfileDir: process.cwd(),
  })).toBeTruthy()
})

test('allProjectsAreUpToDate(): returns false if one of the importers is not present in the lockfile', async () => {
  const fooManifest: DependencyManifest = {
    name: 'foo',
    version: '1.0.0',
    dependencies: {
      'is-odd': '1.0.0',
    },
  }
  const barManifest: DependencyManifest = {
    name: 'bar',
    version: '1.0.0',
    dependencies: {
      'is-even': '1.0.0',
    },
  }
  const workspacePackages: WorkspacePackages = new Map([
    ['foo', new Map([
      ['1.0.0', {
        rootDir: 'foo' as ProjectRootDir,
        manifest: fooManifest,
      }],
    ])],
    ['bar', new Map([
      ['1.0.0', {
        rootDir: 'bar' as ProjectRootDir,
        manifest: barManifest,
      }],
    ])],
  ])
  expect(await allProjectsAreUpToDate([
    {
      id: 'bar' as ProjectId,
      manifest: barManifest,
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
            'is-even': '1.0.0',
          },
          specifiers: {
            'is-even': '1.0.0',
          },
        },
      },
      lockfileVersion: LOCKFILE_VERSION,
    },
    workspacePackages,
    lockfileDir: '',
  })).toBeFalsy()
})

test('allProjectsAreUpToDate(): returns true if one of the importers is not present in the lockfile but the importer has no dependencies', async () => {
  const fooManifest: DependencyManifest = {
    name: 'foo',
    version: '1.0.0',
  }
  const barManifest: DependencyManifest = {
    name: 'bar',
    version: '1.0.0',
    dependencies: {
      'is-even': '1.0.0',
    },
  }
  const workspacePackages: WorkspacePackages = new Map([
    ['foo', new Map([
      ['1.0.0', {
        rootDir: 'foo' as ProjectRootDir,
        manifest: fooManifest,
      }],
    ])],
    ['bar', new Map([
      ['1.0.0', {
        rootDir: 'bar' as ProjectRootDir,
        manifest: barManifest,
      }],
    ])],
  ])
  expect(await allProjectsAreUpToDate([
    {
      id: 'bar' as ProjectId,
      manifest: barManifest,
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
            'is-even': '1.0.0',
          },
          specifiers: {
            'is-even': '1.0.0',
          },
        },
      },
      lockfileVersion: LOCKFILE_VERSION,
    },
    workspacePackages,
    lockfileDir: '',
  })).toBeTruthy()
})

test('allProjectsAreUpToDate(): returns true for injected self-referencing file: dependency resolved as link:', async () => {
  expect(await allProjectsAreUpToDate([
    {
      id: 'can-link' as ProjectId,
      manifest: {
        name: 'can-link',
        version: '2.0.0',
        dependenciesMeta: {
          'can-link': {
            injected: true,
          },
        },
        devDependencies: {
          'can-link': 'file:',
        },
      },
      rootDir: 'can-link' as ProjectRootDir,
    },
  ], {
    autoInstallPeers: false,
    catalogs: {},
    excludeLinksFromLockfile: false,
    linkWorkspacePackages: false,
    wantedLockfile: {
      importers: {
        ['can-link' as ProjectId]: {
          dependenciesMeta: {
            'can-link': {
              injected: true,
            },
          },
          devDependencies: {
            'can-link': 'link:',
          },
          specifiers: {
            'can-link': 'file:',
          },
        },
      },
      lockfileVersion: LOCKFILE_VERSION,
    },
    workspacePackages: new Map(),
    lockfileDir: '',
  })).toBeTruthy()
})

test('allProjectsAreUpToDate(): returns false if the lockfile is broken, the resolved versions do not satisfy the ranges', async () => {
  expect(await allProjectsAreUpToDate([
    {
      id: '.' as ProjectId,
      manifest: {
        dependencies: {
          '@apollo/client': '3.3.7',
        },
      },
      rootDir: '.' as ProjectRootDir,
    },
  ], {
    autoInstallPeers: false,
    catalogs: {},
    excludeLinksFromLockfile: false,
    linkWorkspacePackages: true,
    wantedLockfile: {
      importers: {
        ['.' as ProjectId]: {
          dependencies: {
            '@apollo/client': '3.13.8(@types/react@18.3.23)(graphql@15.8.0)(react-dom@17.0.2(react@17.0.2))(react@17.0.2)(subscriptions-transport-ws@0.11.0(graphql@15.8.0))',
          },
          specifiers: {
            '@apollo/client': '3.3.7',
          },
        },
      },
      lockfileVersion: LOCKFILE_VERSION,
    },
    workspacePackages,
    lockfileDir: '',
  })).toBeFalsy()
})

test('allProjectsAreUpToDate(): works with packages linked through the workspace protocol using home-relative path', async () => {
  const homePkgDir = path.resolve(os.homedir(), 'pkg')
  const relativeFromProject = path.relative(process.cwd(), homePkgDir)
  const project = {
    id: '.' as ProjectId,
    manifest: {
      dependencies: {
        foo: 'workspace:~/pkg',
      },
    },
    rootDir: process.cwd() as ProjectRootDir,
  }
  const opts = {
    autoInstallPeers: false,
    catalogs: {},
    excludeLinksFromLockfile: false,
    linkWorkspacePackages: true,
    wantedLockfile: {
      importers: {
        ['.' as ProjectId]: {
          dependencies: {
            foo: `link:${relativeFromProject}`,
          },
          specifiers: {
            foo: 'workspace:~/pkg',
          },
        },
      },
      lockfileVersion: LOCKFILE_VERSION,
    },
    workspacePackages: new Map(),
    lockfileDir: process.cwd(),
  }
  expect(await allProjectsAreUpToDate([project], opts)).toBeTruthy()

  const mismatchedOpts = {
    ...opts,
    wantedLockfile: {
      ...opts.wantedLockfile,
      importers: {
        ['.' as ProjectId]: {
          dependencies: {
            foo: 'link:./other-path',
          },
          specifiers: {
            foo: 'workspace:~/pkg',
          },
        },
      },
    },
  }
  expect(await allProjectsAreUpToDate([project], mismatchedOpts)).toBeFalsy()
})

test('allProjectsAreUpToDate(): works with nested home-relative workspace dependency in local directory package', async () => {
  prepareEmpty()
  await mkdir('local-dir')
  await writeFile('./local-dir/package.json', JSON.stringify({
    name: 'local-dir',
    version: '1.0.0',
    dependencies: {
      nested: 'workspace:~/pkg',
    },
  }))
  const homePkgDir = path.resolve(os.homedir(), 'pkg')
  const relativeFromLockfile = path.relative(process.cwd(), homePkgDir)
  const nestedProjects = [
    {
      id: 'bar' as ProjectId,
      manifest: {
        dependencies: {
          local: 'file:./local-dir',
        },
      },
      rootDir: 'bar' as ProjectRootDir,
    },
  ]
  const nestedOptions = {
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
      },
      packages: {
        'local@file:./local-dir': {
          resolution: { directory: './local-dir', type: 'directory' },
          version: '1.0.0',
          dependencies: {
            nested: `link:${relativeFromLockfile}`,
          },
          dev: false,
        },
      },
      lockfileVersion: LOCKFILE_VERSION,
    } as LockfileObject,
    workspacePackages: new Map(),
    lockfileDir: process.cwd(),
  }
  expect(await allProjectsAreUpToDate(nestedProjects, nestedOptions)).toBeTruthy()

  const mismatchedNestedOptions = {
    ...nestedOptions,
    wantedLockfile: {
      ...nestedOptions.wantedLockfile,
      packages: {
        'local@file:./local-dir': {
          resolution: { directory: './local-dir', type: 'directory' },
          version: '1.0.0',
          dependencies: {
            nested: 'link:./other-path',
          },
          dev: false,
        },
      },
    } as LockfileObject,
  }
  expect(await allProjectsAreUpToDate(nestedProjects, mismatchedNestedOptions)).toBeFalsy()
})

test('allProjectsAreUpToDate(): works with injected workspace dependency with a file: reference in a package snapshot', async () => {
  prepareEmpty()
  await mkdir('packages/pkg-a', { recursive: true })
  await mkdir('packages/pkg-b', { recursive: true })
  await writeFile('./packages/pkg-a/package.json', JSON.stringify({
    name: 'pkg-a',
    version: '1.0.0',
    dependencies: {
      'pkg-b': 'workspace:*',
    },
  }))
  await writeFile('./packages/pkg-b/package.json', JSON.stringify({
    name: 'pkg-b',
    version: '1.0.0',
  }))
  const projects = [
    {
      id: 'app' as ProjectId,
      manifest: {
        dependencies: {
          'pkg-a': 'file:packages/pkg-a',
        },
      },
      rootDir: 'app' as ProjectRootDir,
    },
  ]
  const opts = {
    autoInstallPeers: false,
    catalogs: {},
    excludeLinksFromLockfile: false,
    linkWorkspacePackages: true,
    wantedLockfile: {
      importers: {
        app: {
          dependencies: {
            'pkg-a': 'file:packages/pkg-a',
          },
          specifiers: {
            'pkg-a': 'file:packages/pkg-a',
          },
        },
      },
      packages: {
        'pkg-a@file:packages/pkg-a': {
          resolution: { directory: 'packages/pkg-a', type: 'directory' },
          version: '1.0.0',
          dependencies: {
            'pkg-b': 'file:packages/pkg-b',
          },
          dev: false,
        },
      },
      lockfileVersion: LOCKFILE_VERSION,
    } as LockfileObject,
    workspacePackages: new Map(),
    lockfileDir: process.cwd(),
  }
  expect(await allProjectsAreUpToDate(projects, opts)).toBeTruthy()
})

// A fresh checkout (or a clean before `--frozen-lockfile`) has nothing at the
// publish directory until the project's own `prepare` script runs, so this
// must not fall back to reading `a`'s manifest from a directory that does
// not exist yet (pnpm/pnpm#7811).
test('allProjectsAreUpToDate(): returns true for an injected workspace dependency that publishes from a directory not yet built', async () => {
  prepareEmpty()
  await mkdir('packages/a', { recursive: true })
  const project1Manifest = {
    name: 'a',
    version: '1.0.0',
    publishConfig: {
      directory: 'dist',
    },
  }
  await writeFile('./packages/a/package.json', JSON.stringify(project1Manifest))
  const projects = [
    {
      id: 'bar' as ProjectId,
      manifest: {
        dependencies: {
          a: 'workspace:1.0.0',
        },
        dependenciesMeta: {
          a: {
            injected: true,
          },
        },
      },
      rootDir: 'bar' as ProjectRootDir,
    },
    {
      id: 'a' as ProjectId,
      manifest: project1Manifest,
      rootDir: path.resolve('packages/a') as ProjectRootDir,
    },
  ]
  const opts = {
    autoInstallPeers: false,
    catalogs: {},
    excludeLinksFromLockfile: false,
    linkWorkspacePackages: true,
    wantedLockfile: {
      importers: {
        bar: {
          dependencies: {
            a: 'file:packages/a/dist',
          },
          specifiers: {
            a: 'workspace:1.0.0',
          },
          dependenciesMeta: {
            a: {
              injected: true,
            },
          },
        },
        a: {
          publishDirectory: 'dist',
          specifiers: {},
        },
      },
      packages: {
        'a@file:packages/a/dist': {
          resolution: { directory: 'packages/a/dist', type: 'directory' },
          version: '1.0.0',
        },
      },
      lockfileVersion: LOCKFILE_VERSION,
    } as LockfileObject,
    workspacePackages: new Map([
      ['a', new Map([
        ['1.0.0', {
          rootDir: path.resolve('packages/a') as ProjectRootDir,
          manifest: project1Manifest,
        }],
      ])],
    ]),
    lockfileDir: process.cwd(),
  }
  expect(await allProjectsAreUpToDate(projects, opts)).toBeTruthy()
})

test('checkLinkedPackagesAreUpToDate(): reports an injected dependency without a package entry even when skipping local directory dependencies', async () => {
  expect(await checkLinkedPackagesAreUpToDate({
    linkWorkspacePackages: true,
    manifestsByDir: {},
    lockfilePackages: {},
    lockfileDir: '',
    skipLocalDirectoryDependencies: true,
  }, {
    dir: 'bar',
    manifest: {
      dependencies: {
        foo: 'workspace:^1.0.0',
      },
      dependenciesMeta: {
        foo: {
          injected: true,
        },
      },
    },
    snapshot: {
      dependencies: {
        foo: 'file:foo',
      },
      specifiers: {
        foo: 'workspace:^1.0.0',
      },
    },
  })).toStrictEqual({
    upToDate: false,
    detailedReason: 'The lockfile has no package entry for local directory dependency "foo" (file:foo)',
  })
})

test('checkLinkedPackagesAreUpToDate(): skips comparing on-disk manifest for deployed file:// directory references', async () => {
  const depPath = 'foo@file:///workspace/packages/foo' as DepPath
  expect(await checkLinkedPackagesAreUpToDate({
    linkWorkspacePackages: true,
    manifestsByDir: {
      [path.resolve('packages/foo')]: {
        name: 'foo',
        version: '1.0.0',
        dependencies: {
          bar: 'catalog:',
        },
      },
    },
    lockfilePackages: {
      [depPath]: {
        resolution: { directory: 'packages/foo', type: 'directory' },
        dependencies: {
          bar: '1.0.0',
        },
      },
    },
    lockfileDir: process.cwd(),
  }, {
    dir: 'deploy',
    manifest: {
      dependencies: {
        foo: depPath,
      },
    },
    snapshot: {
      dependencies: {
        foo: depPath,
      },
      specifiers: {
        foo: depPath,
      },
    },
  })).toStrictEqual({
    upToDate: true,
  })
})

