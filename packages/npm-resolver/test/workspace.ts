/// <reference path="../../../typings/index.d.ts"/>
import path from 'path'
import { createFetchFromRegistry } from '@pnpm/fetch'
import _createResolveFromNpm from '@pnpm/npm-resolver'
import loadJsonFile from 'load-json-file'
import nock from 'nock'
import tempy from 'tempy'

/* eslint-disable @typescript-eslint/no-explicit-any */
const isPositiveMeta = loadJsonFile.sync<any>(path.join(__dirname, 'meta', 'is-positive.json'))
/* eslint-enable @typescript-eslint/no-explicit-any */

const registry = 'https://registry.npmjs.org/'

const fetch = createFetchFromRegistry({})
const getCredentials = () => ({ authHeaderValue: undefined, alwaysAuth: undefined })
const createResolveFromNpm = _createResolveFromNpm.bind(null, fetch, getCredentials)

test('relative workspace protocol is skipped', async () => {
  const cacheDir = tempy.directory()
  const resolve = createResolveFromNpm({
    cacheDir,
  })
  const resolveResult = await resolve({ pref: 'workspace:../is-positive' }, {
    projectDir: '/home/istvan/src',
    registry,
  })

  expect(resolveResult).toBe(null)
})

test('resolve from local directory when it matches the latest version of the package', async () => {
  nock(registry)
    .get('/is-positive')
    .reply(200, isPositiveMeta)

  const cacheDir = tempy.directory()
  const resolve = createResolveFromNpm({
    cacheDir,
  })
  const resolveResult = await resolve({ alias: 'is-positive', pref: '1.0.0' }, {
    projectDir: '/home/istvan/src',
    registry,
    workspacePackages: {
      'is-positive': {
        '1.0.0': {
          dir: '/home/istvan/src/is-positive',
          manifest: {
            name: 'is-positive',
            version: '1.0.0',
          },
        },
      },
    },
  })

  expect(resolveResult!.resolvedVia).toBe('local-filesystem')
  expect(resolveResult!.id).toBe('link:is-positive')
  expect(resolveResult!.latest!.split('.').length).toBe(3)
  expect(resolveResult!.resolution).toStrictEqual({
    directory: '/home/istvan/src/is-positive',
    type: 'directory',
  })
  expect(resolveResult!.manifest).toBeTruthy()
  expect(resolveResult!.manifest!.name).toBe('is-positive')
  expect(resolveResult!.manifest!.version).toBe('1.0.0')
})

test('do not resolve from local directory when alwaysTryWorkspacePackages is false', async () => {
  nock(registry)
    .get('/is-positive')
    .reply(200, isPositiveMeta)

  const cacheDir = tempy.directory()
  const resolve = createResolveFromNpm({
    cacheDir,
  })
  const resolveResult = await resolve({ alias: 'is-positive', pref: '1.0.0' }, {
    alwaysTryWorkspacePackages: false,
    projectDir: '/home/istvan/src',
    registry,
    workspacePackages: {
      'is-positive': {
        '1.0.0': {
          dir: '/home/istvan/src/is-positive',
          manifest: {
            name: 'is-positive',
            version: '1.0.0',
          },
        },
      },
    },
  })

  expect(resolveResult!.resolvedVia).toBe('npm-registry')
  expect(resolveResult!.id).toBe('registry.npmjs.org/is-positive/1.0.0')
  expect(resolveResult!.latest!.split('.').length).toBe(3)
  expect(resolveResult!.resolution).toStrictEqual({
    integrity: 'sha512-9cI+DmhNhA8ioT/3EJFnt0s1yehnAECyIOXdT+2uQGzcEEBaj8oNmVWj33+ZjPndMIFRQh8JeJlEu1uv5/J7pQ==',
    registry,
    tarball: 'https://registry.npmjs.org/is-positive/-/is-positive-1.0.0.tgz',
  })
  expect(resolveResult!.manifest).toBeTruthy()
  expect(resolveResult!.manifest!.name).toBe('is-positive')
  expect(resolveResult!.manifest!.version).toBe('1.0.0')
})

test('resolve from local directory when alwaysTryWorkspacePackages is false but workspace: is used', async () => {
  const cacheDir = tempy.directory()
  const resolve = createResolveFromNpm({
    cacheDir,
  })
  const resolveResult = await resolve({ alias: 'is-positive', pref: 'workspace:*' }, {
    alwaysTryWorkspacePackages: false,
    projectDir: '/home/istvan/src',
    registry,
    workspacePackages: {
      'is-positive': {
        '1.0.0': {
          dir: '/home/istvan/src/is-positive',
          manifest: {
            name: 'is-positive',
            version: '1.0.0',
          },
        },
      },
    },
  })

  expect(resolveResult!.resolvedVia).toBe('local-filesystem')
  expect(resolveResult!.id).toBe('link:is-positive')
  expect(resolveResult!.resolution).toStrictEqual({
    directory: '/home/istvan/src/is-positive',
    type: 'directory',
  })
  expect(resolveResult!.manifest).toBeTruthy()
  expect(resolveResult!.manifest!.name).toBe('is-positive')
  expect(resolveResult!.manifest!.version).toBe('1.0.0')
})

test('resolve from local directory when alwaysTryWorkspacePackages is false but workspace: is used with a different package name', async () => {
  const cacheDir = tempy.directory()
  const resolve = createResolveFromNpm({
    cacheDir,
  })
  const resolveResult = await resolve({ alias: 'positive', pref: 'workspace:is-positive@*' }, {
    alwaysTryWorkspacePackages: false,
    projectDir: '/home/istvan/src',
    registry,
    workspacePackages: {
      'is-positive': {
        '1.0.0': {
          dir: '/home/istvan/src/is-positive',
          manifest: {
            name: 'is-positive',
            version: '1.0.0',
          },
        },
      },
    },
  })

  expect(resolveResult!.resolvedVia).toBe('local-filesystem')
  expect(resolveResult!.id).toBe('link:is-positive')
  expect(resolveResult!.resolution).toStrictEqual({
    directory: '/home/istvan/src/is-positive',
    type: 'directory',
  })
  expect(resolveResult!.manifest).toBeTruthy()
  expect(resolveResult!.manifest!.name).toBe('is-positive')
  expect(resolveResult!.manifest!.version).toBe('1.0.0')
})

test('use version from the registry if it is newer than the local one', async () => {
  nock(registry)
    .get('/is-positive')
    .reply(200, {
      ...isPositiveMeta,
      'dist-tags': { latest: '3.1.0' },
    })

  const resolveFromNpm = createResolveFromNpm({
    cacheDir: tempy.directory(),
  })
  const resolveResult = await resolveFromNpm({
    alias: 'is-positive',
    pref: '^3.0.0',
  }, {
    projectDir: '/home/istvan/src',
    registry,
    workspacePackages: {
      'is-positive': {
        '3.0.0': {
          dir: '/home/istvan/src/is-positive',
          manifest: {
            name: 'is-positive',
            version: '3.0.0',
          },
        },
      },
    },
  })

  expect(resolveResult!.resolvedVia).toBe('npm-registry')
  expect(resolveResult!.id).toBe('registry.npmjs.org/is-positive/3.1.0')
  expect(resolveResult!.latest!.split('.').length).toBe(3)
  expect(resolveResult!.resolution).toStrictEqual({
    integrity: 'sha512-9Qa5b+9n69IEuxk4FiNcavXqkixb9lD03BLtdTeu2bbORnLZQrw+pR/exiSg7SoODeu08yxS47mdZa9ddodNwQ==',
    registry,
    tarball: 'https://registry.npmjs.org/is-positive/-/is-positive-3.1.0.tgz',
  })
  expect(resolveResult!.manifest).toBeTruthy()
  expect(resolveResult!.manifest!.name).toBe('is-positive')
  expect(resolveResult!.manifest!.version).toBe('3.1.0')
})

test('preferWorkspacePackages: use version from the workspace even if there is newer version in the registry', async () => {
  nock(registry)
    .get('/is-positive')
    .reply(200, {
      ...isPositiveMeta,
      'dist-tags': { latest: '3.1.0' },
    })

  const resolveFromNpm = createResolveFromNpm({
    cacheDir: tempy.directory(),
  })
  const resolveResult = await resolveFromNpm({
    alias: 'is-positive',
    pref: '^3.0.0',
  }, {
    preferWorkspacePackages: true,
    projectDir: '/home/istvan/src',
    registry,
    workspacePackages: {
      'is-positive': {
        '3.0.0': {
          dir: '/home/istvan/src/is-positive',
          manifest: {
            name: 'is-positive',
            version: '3.0.0',
          },
        },
      },
    },
  })

  expect(resolveResult).toStrictEqual(
    expect.objectContaining({
      resolvedVia: 'local-filesystem',
      id: 'link:is-positive',
      latest: '3.1.0',
    })
  )
})

test('use local version if it is newer than the latest in the registry', async () => {
  nock(registry)
    .get('/is-positive')
    .reply(200, {
      ...isPositiveMeta,
      'dist-tags': { latest: '3.1.0' },
    })

  const resolveFromNpm = createResolveFromNpm({
    cacheDir: tempy.directory(),
  })
  const resolveResult = await resolveFromNpm({
    alias: 'is-positive',
    pref: '^3.0.0',
  }, {
    projectDir: '/home/istvan/src',
    registry,
    workspacePackages: {
      'is-positive': {
        '3.2.0': {
          dir: '/home/istvan/src/is-positive',
          manifest: {
            name: 'is-positive',
            version: '3.2.0',
          },
        },
      },
    },
  })

  expect(resolveResult!.resolvedVia).toBe('local-filesystem')
  expect(resolveResult!.id).toBe('link:is-positive')
  expect(resolveResult!.latest!.split('.').length).toBe(3)
  expect(resolveResult!.resolution).toStrictEqual({
    directory: '/home/istvan/src/is-positive',
    type: 'directory',
  })
  expect(resolveResult!.manifest).toBeTruthy()
  expect(resolveResult!.manifest!.name).toBe('is-positive')
  expect(resolveResult!.manifest!.version).toBe('3.2.0')
})

test('resolve from local directory when package is not found in the registry', async () => {
  nock(registry)
    .get('/is-positive')
    .reply(404, {})

  const cacheDir = tempy.directory()
  const resolve = createResolveFromNpm({
    cacheDir,
  })
  const resolveResult = await resolve({ alias: 'is-positive', pref: '1' }, {
    projectDir: '/home/istvan/src/foo',
    registry,
    workspacePackages: {
      'is-positive': {
        '1.0.0': {
          dir: '/home/istvan/src/is-positive-1.0.0',
          manifest: {
            name: 'is-positive',
            version: '1.0.0',
          },
        },
        '1.1.0': {
          dir: '/home/istvan/src/is-positive',
          manifest: {
            name: 'is-positive',
            version: '1.1.0',
          },
        },
        '2.0.0': {
          dir: '/home/istvan/src/is-positive-2.0.0',
          manifest: {
            name: 'is-positive',
            version: '2.0.0',
          },
        },
      },
    },
  })

  expect(resolveResult!.resolvedVia).toBe('local-filesystem')
  expect(resolveResult!.id).toBe('link:../is-positive')
  expect(resolveResult!.latest).toBeFalsy()
  expect(resolveResult!.resolution).toStrictEqual({
    directory: '/home/istvan/src/is-positive',
    type: 'directory',
  })
  expect(resolveResult!.manifest).toBeTruthy()
  expect(resolveResult!.manifest!.name).toBe('is-positive')
  expect(resolveResult!.manifest!.version).toBe('1.1.0')
})

test('resolve from local directory when package is not found in the registry and latest installed', async () => {
  nock(registry)
    .get('/is-positive')
    .reply(404, {})

  const cacheDir = tempy.directory()
  const resolve = createResolveFromNpm({
    cacheDir,
  })
  const resolveResult = await resolve({ alias: 'is-positive', pref: 'latest' }, {
    projectDir: '/home/istvan/src',
    registry,
    workspacePackages: {
      'is-positive': {
        '1.0.0': {
          dir: '/home/istvan/src/is-positive-1.0.0',
          manifest: {
            name: 'is-positive',
            version: '1.0.0',
          },
        },
        '1.1.0': {
          dir: '/home/istvan/src/is-positive',
          manifest: {
            name: 'is-positive',
            version: '1.1.0',
          },
        },
        '2.0.0': {
          dir: '/home/istvan/src/is-positive-2.0.0',
          manifest: {
            name: 'is-positive',
            version: '2.0.0',
          },
        },
      },
    },
  })

  expect(resolveResult!.resolvedVia).toBe('local-filesystem')
  expect(resolveResult!.id).toBe('link:is-positive-2.0.0')
  expect(resolveResult!.latest).toBeFalsy()
  expect(resolveResult!.resolution).toStrictEqual({
    directory: '/home/istvan/src/is-positive-2.0.0',
    type: 'directory',
  })
  expect(resolveResult!.manifest).toBeTruthy()
  expect(resolveResult!.manifest!.name).toBe('is-positive')
  expect(resolveResult!.manifest!.version).toBe('2.0.0')
})

test('resolve from local directory when package is not found in the registry and specific version is requested', async () => {
  nock(registry)
    .get('/is-positive')
    .reply(404, {})

  const cacheDir = tempy.directory()
  const resolve = createResolveFromNpm({
    cacheDir,
  })
  const resolveResult = await resolve({ alias: 'is-positive', pref: '1.1.0' }, {
    projectDir: '/home/istvan/src/foo',
    registry,
    workspacePackages: {
      'is-positive': {
        '1.0.0': {
          dir: '/home/istvan/src/is-positive-1.0.0',
          manifest: {
            name: 'is-positive',
            version: '1.0.0',
          },
        },
        '1.1.0': {
          dir: '/home/istvan/src/is-positive',
          manifest: {
            name: 'is-positive',
            version: '1.1.0',
          },
        },
        '2.0.0': {
          dir: '/home/istvan/src/is-positive-2.0.0',
          manifest: {
            name: 'is-positive',
            version: '2.0.0',
          },
        },
      },
    },
  })

  expect(resolveResult!.resolvedVia).toBe('local-filesystem')
  expect(resolveResult!.id).toBe('link:../is-positive')
  expect(resolveResult!.latest).toBeFalsy()
  expect(resolveResult!.resolution).toStrictEqual({
    directory: '/home/istvan/src/is-positive',
    type: 'directory',
  })
  expect(resolveResult!.manifest).toBeTruthy()
  expect(resolveResult!.manifest!.name).toBe('is-positive')
  expect(resolveResult!.manifest!.version).toBe('1.1.0')
})

test('resolve from local directory when the requested version is not found in the registry but is available locally', async () => {
  nock(registry)
    .get('/is-positive')
    .reply(200, isPositiveMeta)

  const cacheDir = tempy.directory()
  const resolve = createResolveFromNpm({
    cacheDir,
  })
  const resolveResult = await resolve({ alias: 'is-positive', pref: '100.0.0' }, {
    projectDir: '/home/istvan/src/foo',
    registry,
    workspacePackages: {
      'is-positive': {
        '100.0.0': {
          dir: '/home/istvan/src/is-positive',
          manifest: {
            name: 'is-positive',
            version: '100.0.0',
          },
        },
      },
    },
  })

  expect(resolveResult!.resolvedVia).toBe('local-filesystem')
  expect(resolveResult!.id).toBe('link:../is-positive')
  expect(resolveResult!.latest).toBeFalsy()
  expect(resolveResult!.resolution).toStrictEqual({
    directory: '/home/istvan/src/is-positive',
    type: 'directory',
  })
  expect(resolveResult!.manifest).toBeTruthy()
  expect(resolveResult!.manifest!.name).toBe('is-positive')
  expect(resolveResult!.manifest!.version).toBe('100.0.0')
})

test('workspace protocol: resolve from local directory even when it does not match the latest version of the package', async () => {
  const cacheDir = tempy.directory()
  const resolve = createResolveFromNpm({
    cacheDir,
  })
  const resolveResult = await resolve({ alias: 'is-positive', pref: 'workspace:^3.0.0' }, {
    projectDir: '/home/istvan/src',
    registry,
    workspacePackages: {
      'is-positive': {
        '3.0.0': {
          dir: '/home/istvan/src/is-positive',
          manifest: {
            name: 'is-positive',
            version: '3.0.0',
          },
        },
      },
    },
  })

  expect(resolveResult!.resolvedVia).toBe('local-filesystem')
  expect(resolveResult!.id).toBe('link:is-positive')
  expect(resolveResult!.latest).toBeFalsy()
  expect(resolveResult!.resolution).toStrictEqual({
    directory: '/home/istvan/src/is-positive',
    type: 'directory',
  })
  expect(resolveResult!.manifest).toBeTruthy()
  expect(resolveResult!.manifest!.name).toBe('is-positive')
  expect(resolveResult!.manifest!.version).toBe('3.0.0')
})

test('workspace protocol: resolve from local package that has a pre-release version', async () => {
  nock(registry)
    .get('/is-positive')
    .reply(200, isPositiveMeta)

  const cacheDir = tempy.directory()
  const resolve = createResolveFromNpm({
    cacheDir,
  })
  const resolveResult = await resolve({ alias: 'is-positive', pref: 'workspace:*' }, {
    projectDir: '/home/istvan/src',
    registry,
    workspacePackages: {
      'is-positive': {
        '3.0.0-alpha.1.2.3': {
          dir: '/home/istvan/src/is-positive',
          manifest: {
            name: 'is-positive',
            version: '3.0.0-alpha.1.2.3',
          },
        },
      },
    },
  })

  expect(resolveResult!.resolvedVia).toBe('local-filesystem')
  expect(resolveResult!.id).toBe('link:is-positive')
  expect(resolveResult!.latest).toBeFalsy()
  expect(resolveResult!.resolution).toStrictEqual({
    directory: '/home/istvan/src/is-positive',
    type: 'directory',
  })
  expect(resolveResult!.manifest).toBeTruthy()
  expect(resolveResult!.manifest!.name).toBe('is-positive')
  expect(resolveResult!.manifest!.version).toBe('3.0.0-alpha.1.2.3')
})

test("workspace protocol: don't resolve from local package that has a pre-release version that don't satisfy the range", async () => {
  nock(registry)
    .get('/is-positive')
    .reply(200, isPositiveMeta)

  const cacheDir = tempy.directory()
  const resolve = createResolveFromNpm({
    cacheDir,
  })
  const resolveResult = await resolve({ alias: 'is-positive', pref: '2' }, {
    projectDir: '/home/istvan/src',
    registry,
    workspacePackages: {
      'is-positive': {
        '3.0.0-alpha.1.2.3': {
          dir: '/home/istvan/src/is-positive',
          manifest: {
            name: 'is-positive',
            version: '3.0.0-alpha.1.2.3',
          },
        },
      },
    },
  })

  expect(resolveResult!.resolvedVia).toBe('npm-registry')
  expect(resolveResult!.id).toBe('registry.npmjs.org/is-positive/2.0.0')
  expect(resolveResult!.latest).toBeTruthy()
  expect(resolveResult!.manifest).toBeTruthy()
  expect(resolveResult!.manifest!.name).toBe('is-positive')
  expect(resolveResult!.manifest!.version).toBe('2.0.0')
})

test('workspace protocol: resolution fails if there is no matching local package', async () => {
  const cacheDir = tempy.directory()
  const resolve = createResolveFromNpm({
    cacheDir,
  })

  const projectDir = '/home/istvan/src'
  let err!: Error
  try {
    await resolve({ alias: 'is-positive', pref: 'workspace:^3.0.0' }, {
      projectDir,
      registry,
      workspacePackages: {},
    })
  } catch (_err: any) { // eslint-disable-line
    err = _err
  }

  expect(err).toBeTruthy()
  expect(err['code']).toBe('ERR_PNPM_NO_MATCHING_VERSION_INSIDE_WORKSPACE')
  expect(err.message).toBe(`In ${path.relative(process.cwd(), projectDir)}: No matching version found for is-positive@^3.0.0 inside the workspace`)
})

test('workspace protocol: resolution fails if there are no local packages', async () => {
  const cacheDir = tempy.directory()
  const resolve = createResolveFromNpm({
    cacheDir,
  })

  let err!: Error
  try {
    await resolve({ alias: 'is-positive', pref: 'workspace:^3.0.0' }, {
      projectDir: '/home/istvan/src',
      registry,
    })
  } catch (_err: any) { // eslint-disable-line
    err = _err
  }

  expect(err).toBeTruthy()
  expect(err.message).toBe('Cannot resolve package from workspace because opts.workspacePackages is not defined')
})

test('resolve workspace:^', async () => {
  const cacheDir = tempy.directory()
  const resolve = createResolveFromNpm({
    cacheDir,
  })
  const resolveResult = await resolve({ alias: 'is-positive', pref: 'workspace:^' }, {
    projectDir: '/home/istvan/src',
    registry,
    workspacePackages: {
      'is-positive': {
        '1.0.0': {
          dir: '/home/istvan/src/is-positive',
          manifest: {
            name: 'is-positive',
            version: '1.0.0',
          },
        },
      },
    },
  })

  expect(resolveResult!.resolvedVia).toBe('local-filesystem')
  expect(resolveResult!.id).toBe('link:is-positive')
  expect(resolveResult!.resolution).toStrictEqual({
    directory: '/home/istvan/src/is-positive',
    type: 'directory',
  })
  expect(resolveResult!.manifest).toBeTruthy()
  expect(resolveResult!.manifest!.name).toBe('is-positive')
  expect(resolveResult!.manifest!.version).toBe('1.0.0')
})

test('resolve workspace:~', async () => {
  const cacheDir = tempy.directory()
  const resolve = createResolveFromNpm({
    cacheDir,
  })
  const resolveResult = await resolve({ alias: 'is-positive', pref: 'workspace:~' }, {
    projectDir: '/home/istvan/src',
    registry,
    workspacePackages: {
      'is-positive': {
        '1.0.0': {
          dir: '/home/istvan/src/is-positive',
          manifest: {
            name: 'is-positive',
            version: '1.0.0',
          },
        },
      },
    },
  })

  expect(resolveResult!.resolvedVia).toBe('local-filesystem')
  expect(resolveResult!.id).toBe('link:is-positive')
  expect(resolveResult!.resolution).toStrictEqual({
    directory: '/home/istvan/src/is-positive',
    type: 'directory',
  })
  expect(resolveResult!.manifest).toBeTruthy()
  expect(resolveResult!.manifest!.name).toBe('is-positive')
  expect(resolveResult!.manifest!.version).toBe('1.0.0')
})

test('resolve from local directory when package name have scope with * pref', async () => {
  const cacheDir = tempy.directory()
  const resolve = createResolveFromNpm({
    cacheDir,
  })
  const resolveResult = await resolve({ alias: 'positive', pref: 'workspace:@scope/is-positive@*' }, {
    projectDir: '/home/istvan/src',
    registry,
    workspacePackages: {
      '@scope/is-positive': {
        '1.0.0': {
          dir: '/home/istvan/src/is-positive',
          manifest: {
            name: '@scope/is-positive',
            version: '1.0.0',
          },
        },
      },
    },
  })

  expect(resolveResult!.resolvedVia).toBe('local-filesystem')
  expect(resolveResult!.id).toBe('link:is-positive')
  expect(resolveResult!.resolution).toStrictEqual({
    directory: '/home/istvan/src/is-positive',
    type: 'directory',
  })
  expect(resolveResult!.manifest).toBeTruthy()
  expect(resolveResult!.manifest!.name).toBe('@scope/is-positive')
  expect(resolveResult!.manifest!.version).toBe('1.0.0')
})

test('resolve from local directory when package name have scope with a regular semver pref', async () => {
  const cacheDir = tempy.directory()
  const resolve = createResolveFromNpm({
    cacheDir,
  })
  const resolveResult = await resolve({ alias: 'positive', pref: 'workspace:@scope/is-positive@^1.0.0' }, {
    projectDir: '/home/istvan/src',
    registry,
    workspacePackages: {
      '@scope/is-positive': {
        '1.0.0': {
          dir: '/home/istvan/src/is-positive',
          manifest: {
            name: '@scope/is-positive',
            version: '1.0.0',
          },
        },
      },
    },
  })

  expect(resolveResult!.resolvedVia).toBe('local-filesystem')
  expect(resolveResult!.id).toBe('link:is-positive')
  expect(resolveResult!.resolution).toStrictEqual({
    directory: '/home/istvan/src/is-positive',
    type: 'directory',
  })
  expect(resolveResult!.manifest).toBeTruthy()
  expect(resolveResult!.manifest!.name).toBe('@scope/is-positive')
  expect(resolveResult!.manifest!.version).toBe('1.0.0')
})

test('resolve from local directory when package name have scope with a workspace only semver pref', async () => {
  const cacheDir = tempy.directory()
  const resolve = createResolveFromNpm({
    cacheDir,
  })
  const resolveResult = await resolve({ alias: 'positive', pref: 'workspace:@scope/is-positive@~' }, {
    projectDir: '/home/istvan/src',
    registry,
    workspacePackages: {
      '@scope/is-positive': {
        '1.0.0': {
          dir: '/home/istvan/src/is-positive',
          manifest: {
            name: '@scope/is-positive',
            version: '1.0.0',
          },
        },
      },
    },
  })

  expect(resolveResult!.resolvedVia).toBe('local-filesystem')
  expect(resolveResult!.id).toBe('link:is-positive')
  expect(resolveResult!.resolution).toStrictEqual({
    directory: '/home/istvan/src/is-positive',
    type: 'directory',
  })
  expect(resolveResult!.manifest).toBeTruthy()
  expect(resolveResult!.manifest!.name).toBe('@scope/is-positive')
  expect(resolveResult!.manifest!.version).toBe('1.0.0')
})
