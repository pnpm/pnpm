import { LOCKFILE_VERSION } from '@pnpm/constants'
import { prepareEmpty } from '@pnpm/prepare'
import { type WorkspacePackages } from '@pnpm/resolver-base'
import { type DepPath, type DependencyManifest, type ProjectId, type ProjectRootDir } from '@pnpm/types'
import { allProjectsAreUpToDate } from '@pnpm/lockfile.verification'
import { createWriteStream } from 'fs'
import { writeFile, mkdir } from 'fs/promises'
import { type LockfileObject } from '@pnpm/lockfile.types'
import tar from 'tar-stream'
import { pipeline } from 'stream/promises'
import { getTarballIntegrity } from '@pnpm/crypto.hash'

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

  test('allProjectsAreUpToDate(): returns false if local dep does not exist', async () => {
    expect.hasAssertions()

    const lockfileDir = process.cwd()
    expect(await allProjectsAreUpToDate(projects, { ...options, lockfileDir })).toBeFalsy()
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
