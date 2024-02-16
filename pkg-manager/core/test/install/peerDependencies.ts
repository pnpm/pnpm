import fs from 'fs'
import path from 'path'
import { WANTED_LOCKFILE } from '@pnpm/constants'
import { type Lockfile } from '@pnpm/lockfile-file'
import { prepareEmpty, preparePackages } from '@pnpm/prepare'
import { addDistTag, REGISTRY_MOCK_PORT } from '@pnpm/registry-mock'
import { fixtures } from '@pnpm/test-fixtures'
import { sync as readYamlFile } from 'read-yaml-file'
import {
  addDependenciesToPackage,
  install,
  type MutatedProject,
  mutateModules,
  mutateModulesInSingleProject,
  type PeerDependencyIssuesError,
  type ProjectOptions,
} from '@pnpm/core'
import { sync as rimraf } from '@zkochan/rimraf'
import sinon from 'sinon'
import deepRequireCwd from 'deep-require-cwd'
import { createPeersDirSuffix, depPathToFilename } from '@pnpm/dependency-path'
import { testDefaults } from '../utils'

const f = fixtures(__dirname)

test("don't fail when peer dependency is fetched from GitHub", async () => {
  prepareEmpty()
  await addDependenciesToPackage({}, ['@pnpm.e2e/test-pnpm-peer-deps'], await testDefaults())
})

test('peer dependency is grouped with dependency when peer is resolved not from a top dependency 1', async () => {
  const project = prepareEmpty()
  const opts = await testDefaults()
  let manifest = await addDependenciesToPackage({}, ['@pnpm.e2e/using-ajv'], opts)

  expect(fs.existsSync(path.resolve('node_modules/.pnpm/ajv-keywords@1.5.0_ajv@4.10.4/node_modules/ajv'))).toBeTruthy()
  expect(deepRequireCwd(['@pnpm.e2e/using-ajv', 'ajv-keywords', 'ajv', './package.json']).version).toBe('4.10.4')

  // testing that peers are reinstalled correctly using info from the lockfile
  rimraf('node_modules')
  rimraf(path.resolve('..', '.store'))
  manifest = await install(manifest, await testDefaults())

  expect(fs.existsSync(path.resolve('node_modules/.pnpm/ajv-keywords@1.5.0_ajv@4.10.4/node_modules/ajv'))).toBeTruthy()
  expect(deepRequireCwd(['@pnpm.e2e/using-ajv', 'ajv-keywords', 'ajv', './package.json']).version).toBe('4.10.4')

  await addDependenciesToPackage(manifest, ['@pnpm.e2e/using-ajv'], await testDefaults({ update: true }))

  const lockfile = project.readLockfile()

  expect(lockfile.packages['/@pnpm.e2e/using-ajv@1.0.0'].dependencies!['ajv-keywords']).toBe('1.5.0(ajv@4.10.4)')
  // covers https://github.com/pnpm/pnpm/issues/1150
  expect(lockfile.packages).toHaveProperty(['/ajv-keywords@1.5.0(ajv@4.10.4)'])
})

// Covers https://github.com/pnpm/pnpm/issues/1133
test('nothing is needlessly removed from node_modules', async () => {
  prepareEmpty()
  const opts = await testDefaults({
    autoInstallPeers: false,
    dedupePeerDependents: false,
    modulesCacheMaxAge: 0,
    strictPeerDependencies: false,
  })
  const manifest = await addDependenciesToPackage({}, ['@pnpm.e2e/using-ajv', 'ajv-keywords@1.5.0'], opts)

  expect(fs.existsSync(path.resolve('node_modules/.pnpm/ajv-keywords@1.5.0_ajv@4.10.4/node_modules/ajv'))).toBeTruthy()
  expect(fs.existsSync(path.resolve('node_modules/.pnpm/ajv-keywords@1.5.0/node_modules/ajv-keywords'))).toBeTruthy()
  expect(deepRequireCwd(['@pnpm.e2e/using-ajv', 'ajv-keywords', 'ajv', './package.json']).version).toBe('4.10.4')

  await mutateModulesInSingleProject({
    dependencyNames: ['ajv-keywords'],
    manifest,
    mutation: 'uninstallSome',
    rootDir: process.cwd(),
  }, opts)

  expect(fs.existsSync(path.resolve('node_modules/.pnpm/ajv-keywords@1.5.0_ajv@4.10.4/node_modules/ajv'))).toBeTruthy()
  expect(fs.existsSync(path.resolve('node_modules/.pnpm/ajv-keywords@1.5.0/node_modules/ajv-keywords'))).toBeFalsy()
})

test('peer dependency is grouped with dependent when the peer is a top dependency', async () => {
  const project = prepareEmpty()

  const reporter = sinon.spy()

  const manifest = await addDependenciesToPackage({}, ['ajv@4.10.4', 'ajv-keywords@1.5.0'], await testDefaults({ reporter }))

  expect(reporter.calledWithMatch({
    message: `localhost+${REGISTRY_MOCK_PORT}/ajv-keywords/1.5.0 requires a peer of ajv@>=4.10.0 but none was installed.`,
  })).toBeFalsy()

  expect(fs.existsSync(path.resolve('node_modules/.pnpm/ajv-keywords@1.5.0_ajv@4.10.4/node_modules/ajv-keywords'))).toBeTruthy()

  await mutateModulesInSingleProject({
    manifest,
    mutation: 'install',
    rootDir: process.cwd(),
  }, await testDefaults({ preferFrozenLockfile: false }))

  const lockfile = project.readLockfile()
  expect(lockfile.packages['/ajv-keywords@1.5.0(ajv@4.10.4)'].dependencies).toHaveProperty(['ajv'])
})

test('the right peer dependency is used in every workspace package', async () => {
  const manifest1 = {
    name: 'project-1',

    dependencies: {
      'ajv-keywords': '1.5.0',
    },
  }
  const manifest2 = {
    name: 'project-2',

    dependencies: {
      ajv: '4.10.4',
      'ajv-keywords': '1.5.0',
    },
  }
  preparePackages([
    {
      location: 'project-1',
      package: manifest1,
    },
    {
      location: 'project-2',
      package: manifest2,
    },
  ])

  const importers: MutatedProject[] = [
    {
      mutation: 'install',
      rootDir: path.resolve('project-1'),
    },
    {
      mutation: 'install',
      rootDir: path.resolve('project-2'),
    },
  ]
  const allProjects = [
    {
      buildIndex: 0,
      manifest: manifest1,
      rootDir: path.resolve('project-1'),
    },
    {
      buildIndex: 0,
      manifest: manifest2,
      rootDir: path.resolve('project-2'),
    },
  ]
  await mutateModules(importers, await testDefaults({
    allProjects,
    autoInstallPeers: false,
    dedupePeerDependents: false,
    lockfileOnly: true,
    strictPeerDependencies: false,
  }))

  const lockfile = readYamlFile<Lockfile>(path.resolve(WANTED_LOCKFILE))

  expect(lockfile.importers['project-1'].dependencies).toStrictEqual({
    'ajv-keywords': {
      specifier: '1.5.0',
      version: '1.5.0',
    },
  })
  expect(lockfile.importers['project-2'].dependencies).toStrictEqual({
    ajv: {
      specifier: '4.10.4',
      version: '4.10.4',
    },
    'ajv-keywords': {
      specifier: '1.5.0',
      version: '1.5.0(ajv@4.10.4)',
    },
  })
})

test('warning is reported when cannot resolve peer dependency for top-level dependency', async () => {
  prepareEmpty()

  const reporter = jest.fn()

  await addDependenciesToPackage(
    {},
    ['ajv-keywords@1.5.0'],
    await testDefaults({ autoInstallPeers: false, reporter, strictPeerDependencies: false })
  )

  expect(reporter).toHaveBeenCalledWith(
    expect.objectContaining({
      level: 'debug',
      name: 'pnpm:peer-dependency-issues',
      issuesByProjects: {
        '.': {
          bad: {},
          missing: {
            ajv: [
              {
                parents: [
                  {
                    name: 'ajv-keywords',
                    version: '1.5.0',
                  },
                ],
                optional: false,
                wantedRange: '>=4.10.0',
              },
            ],
          },
          conflicts: [],
          intersections: { ajv: '>=4.10.0' },
        },
      },
    })
  )
})

test('strict-peer-dependencies: error is thrown when cannot resolve peer dependency for top-level dependency', async () => {
  prepareEmpty()

  let err!: PeerDependencyIssuesError
  try {
    await install({
      dependencies: {
        'ajv-keywords': '1.5.0',
      },
    }, await testDefaults({ autoInstallPeers: false, strictPeerDependencies: true }))
  } catch (_err: any) { // eslint-disable-line
    err = _err
  }

  expect(err?.issuesByProjects['.']).toStrictEqual({
    bad: {},
    missing: {
      ajv: [
        {
          parents: [
            {
              name: 'ajv-keywords',
              version: '1.5.0',
            },
          ],
          optional: false,
          wantedRange: '>=4.10.0',
        },
      ],
    },
    conflicts: [],
    intersections: { ajv: '>=4.10.0' },
  })
})

test('peer dependency is resolved from the dependencies of the workspace root project', async () => {
  const projects = preparePackages([
    {
      location: '.',
      package: { name: 'root' },
    },
    {
      location: 'pkg',
      package: {},
    },
  ])
  const allProjects: ProjectOptions[] = [
    {
      buildIndex: 0,
      manifest: {
        name: 'root',
        version: '1.0.0',

        dependencies: {
          ajv: '4.10.0',
        },
      },
      rootDir: process.cwd(),
    },
    {
      buildIndex: 0,
      manifest: {
        name: 'pkg',
        version: '1.0.0',

        dependencies: {
          'ajv-keywords': '1.5.0',
        },
      },
      rootDir: path.resolve('pkg'),
    },
  ]
  const reporter = jest.fn()
  await mutateModules([
    {
      mutation: 'install',
      rootDir: process.cwd(),
    },
    {
      mutation: 'install',
      rootDir: path.resolve('pkg'),
    },
  ], await testDefaults({ allProjects, reporter, resolvePeersFromWorkspaceRoot: true }))

  expect(reporter).not.toHaveBeenCalledWith(expect.objectContaining({
    name: 'pnpm:peer-dependency-issues',
  }))

  {
    const lockfile = projects.root.readLockfile()
    expect(lockfile.importers.pkg?.dependencies?.['ajv-keywords'].version).toBe('1.5.0(ajv@4.10.0)')
  }

  allProjects[1].manifest.dependencies!['is-positive'] = '1.0.0'
  await mutateModules([
    {
      mutation: 'install',
      rootDir: process.cwd(),
    },
    {
      mutation: 'install',
      rootDir: path.resolve('pkg'),
    },
  ], await testDefaults({ allProjects, reporter, resolvePeersFromWorkspaceRoot: true }))

  {
    const lockfile = projects.root.readLockfile()
    expect(lockfile.importers.pkg?.dependencies?.['ajv-keywords'].version).toBe('1.5.0(ajv@4.10.0)')
  }
})

test('warning is reported when cannot resolve peer dependency for non-top-level dependency', async () => {
  prepareEmpty()
  await addDistTag({ package: '@pnpm.e2e/abc-parent-with-ab', version: '1.0.0', distTag: 'latest' })

  const reporter = jest.fn()

  await addDependenciesToPackage(
    {},
    ['@pnpm.e2e/abc-grand-parent-without-c'],
    await testDefaults({ autoInstallPeers: false, reporter, strictPeerDependencies: false })
  )

  expect(reporter).toHaveBeenCalledWith(
    expect.objectContaining({
      level: 'debug',
      name: 'pnpm:peer-dependency-issues',
      issuesByProjects: {
        '.': {
          bad: {},
          missing: {
            '@pnpm.e2e/peer-c': [
              {
                parents: [
                  {
                    name: '@pnpm.e2e/abc-grand-parent-without-c',
                    version: '1.0.0',
                  },
                  {
                    name: '@pnpm.e2e/abc-parent-with-ab',
                    version: '1.0.0',
                  },
                  {
                    name: '@pnpm.e2e/abc',
                    version: '1.0.0',
                  },
                ],
                optional: false,
                wantedRange: '^1.0.0',
              },
            ],
          },
          conflicts: [],
          intersections: { '@pnpm.e2e/peer-c': '^1.0.0' },
        },
      },
    })
  )
})

test('warning is reported when bad version of resolved peer dependency for non-top-level dependency', async () => {
  await addDistTag({ package: '@pnpm.e2e/abc-parent-with-ab', version: '1.0.0', distTag: 'latest' })
  prepareEmpty()

  const reporter = jest.fn()

  await addDependenciesToPackage({}, ['@pnpm.e2e/abc-grand-parent-without-c', '@pnpm.e2e/peer-c@2'], await testDefaults({ reporter, strictPeerDependencies: false }))

  expect(reporter).toHaveBeenCalledWith(
    expect.objectContaining({
      level: 'debug',
      name: 'pnpm:peer-dependency-issues',
      issuesByProjects: {
        '.': {
          bad: {
            '@pnpm.e2e/peer-c': [
              {
                parents: [
                  {
                    name: '@pnpm.e2e/abc-grand-parent-without-c',
                    version: '1.0.0',
                  },
                  {
                    name: '@pnpm.e2e/abc-parent-with-ab',
                    version: '1.0.0',
                  },
                  {
                    name: '@pnpm.e2e/abc',
                    version: '1.0.0',
                  },
                ],
                foundVersion: '2.0.0',
                resolvedFrom: [],
                optional: false,
                wantedRange: '^1.0.0',
              },
            ],
          },
          missing: {},
          conflicts: [],
          intersections: {},
        },
      },
    })
  )
})

test('strict-peer-dependencies: error is thrown when bad version of resolved peer dependency for non-top-level dependency', async () => {
  await addDistTag({ package: '@pnpm.e2e/abc-parent-with-ab', version: '1.0.0', distTag: 'latest' })
  prepareEmpty()

  let err!: PeerDependencyIssuesError
  try {
    await install({
      dependencies: {
        '@pnpm.e2e/abc-grand-parent-without-c': '1.0.0',
        '@pnpm.e2e/peer-c': '2',
      },
    }, await testDefaults({ strictPeerDependencies: true }))
  } catch (_err: any) { // eslint-disable-line
    err = _err
  }

  expect(err?.issuesByProjects['.']).toStrictEqual({
    bad: {
      '@pnpm.e2e/peer-c': [
        {
          parents: [
            {
              name: '@pnpm.e2e/abc-grand-parent-without-c',
              version: '1.0.0',
            },
            {
              name: '@pnpm.e2e/abc-parent-with-ab',
              version: '1.0.0',
            },
            {
              name: '@pnpm.e2e/abc',
              version: '1.0.0',
            },
          ],
          foundVersion: '2.0.0',
          resolvedFrom: [],
          optional: false,
          wantedRange: '^1.0.0',
        },
      ],
    },
    missing: {},
    conflicts: [],
    intersections: {},
  })
})

test('top peer dependency is linked on subsequent install', async () => {
  prepareEmpty()

  const manifest = await addDependenciesToPackage({}, ['@pnpm.e2e/peer-c@1.0.0'], await testDefaults())

  await addDependenciesToPackage(manifest, ['@pnpm.e2e/abc-parent-with-ab@1.0.0'], await testDefaults())

  expect(fs.existsSync(path.resolve('node_modules/.pnpm/@pnpm.e2e+abc-parent-with-ab@1.0.0/node_modules/@pnpm.e2e/abc-parent-with-ab'))).toBeFalsy()
  expect(fs.existsSync(path.resolve('node_modules/.pnpm/@pnpm.e2e+abc-parent-with-ab@1.0.0_@pnpm.e2e+peer-c@1.0.0/node_modules/@pnpm.e2e/abc-parent-with-ab'))).toBeTruthy()
})

test('top peer dependency is linked on subsequent install, through transitive peer', async () => {
  prepareEmpty()

  const manifest = await addDependenciesToPackage({}, ['@pnpm.e2e/abc-grand-parent@1.0.0'], await testDefaults({ strictPeerDependencies: false }))

  await addDependenciesToPackage(manifest, ['@pnpm.e2e/peer-c@1.0.0'], await testDefaults({ strictPeerDependencies: false }))

  expect(fs.existsSync(path.resolve('node_modules/.pnpm/@pnpm.e2e+abc-grand-parent@1.0.0_@pnpm.e2e+peer-c@1.0.0/node_modules/@pnpm.e2e/abc-grand-parent'))).toBeTruthy()
})

test('the list of transitive peer dependencies is kept up to date', async () => {
  const project = prepareEmpty()
  await addDistTag({ package: '@pnpm.e2e/abc-parent-with-ab', version: '1.0.0', distTag: 'latest' })

  const manifest = await addDependenciesToPackage({}, ['@pnpm.e2e/abc-grand-parent@1.0.0', '@pnpm.e2e/peer-c@1.0.0'], await testDefaults())

  await addDistTag({ package: '@pnpm.e2e/abc-parent-with-ab', version: '1.1.0', distTag: 'latest' })

  expect(fs.existsSync(path.resolve('node_modules/.pnpm/@pnpm.e2e+abc-grand-parent@1.0.0_@pnpm.e2e+peer-c@1.0.0/node_modules/@pnpm.e2e/abc-grand-parent'))).toBeTruthy()
  {
    const lockfile = project.readLockfile()
    expect(lockfile.packages['/@pnpm.e2e/abc-grand-parent@1.0.0(@pnpm.e2e/peer-c@1.0.0)'].transitivePeerDependencies).toStrictEqual(['@pnpm.e2e/peer-c'])
  }

  await mutateModulesInSingleProject({
    manifest,
    mutation: 'install',
    rootDir: process.cwd(),
  }, await testDefaults({ update: true, depth: Infinity }))

  expect(fs.existsSync(path.resolve('node_modules/.pnpm/@pnpm.e2e+abc-grand-parent@1.0.0/node_modules/@pnpm.e2e/abc-grand-parent'))).toBeTruthy()

  {
    const lockfile = project.readLockfile()
    expect(lockfile.packages['/@pnpm.e2e/abc-grand-parent@1.0.0'].transitivePeerDependencies).toBeFalsy()
  }
})

test('top peer dependency is linked on subsequent install. Reverse order', async () => {
  prepareEmpty()

  const manifest = await addDependenciesToPackage({}, ['@pnpm.e2e/abc-parent-with-ab@1.0.0'], await testDefaults({ strictPeerDependencies: false }))

  await addDependenciesToPackage(manifest, ['@pnpm.e2e/peer-c@1.0.0'], await testDefaults({ modulesCacheMaxAge: 0, strictPeerDependencies: false }))

  expect(fs.existsSync(path.resolve('node_modules/.pnpm/@pnpm.e2e+abc-parent-with-ab@1.0.0/node_modules/@pnpm.e2e/abc-parent-with-ab'))).toBeFalsy()
  expect(fs.existsSync(path.resolve('node_modules/.pnpm/@pnpm.e2e+abc-parent-with-ab@1.0.0_@pnpm.e2e+peer-c@1.0.0/node_modules/@pnpm.e2e/abc-parent-with-ab'))).toBeTruthy()
  expect(fs.existsSync(path.resolve('node_modules/.pnpm/@pnpm.e2e+abc-parent-with-ab@1.0.0_@pnpm.e2e+peer-c@1.0.0/node_modules/is-positive'))).toBeTruthy()
})

async function okFile (filename: string) {
  expect(fs.existsSync(filename)).toBeTruthy()
}

// This usecase was failing. See https://github.com/pnpm/supi/issues/15
test('peer dependencies are linked when running one named installation', async () => {
  await addDistTag({ package: '@pnpm.e2e/abc-parent-with-ab', version: '1.0.1', distTag: 'latest' })
  await addDistTag({ package: '@pnpm.e2e/peer-a', version: '1.0.0', distTag: 'latest' })
  await addDistTag({ package: '@pnpm.e2e/peer-c', version: '1.0.0', distTag: 'latest' })

  prepareEmpty()

  const manifest = await addDependenciesToPackage({}, ['@pnpm.e2e/abc-grand-parent-with-c', '@pnpm.e2e/abc-parent-with-ab', '@pnpm.e2e/peer-c@2.0.0'], await testDefaults({ autoInstallPeers: false, strictPeerDependencies: false }))

  const pkgVariation1 = path.join(
    'node_modules/.pnpm',
    depPathToFilename(`/@pnpm.e2e/abc@1.0.0${createPeersDirSuffix([{ name: '@pnpm.e2e/peer-a', version: '1.0.0' }, { name: '@pnpm.e2e/peer-b', version: '1.0.0' }, { name: '@pnpm.e2e/peer-c', version: '1.0.0' }])}`),
    'node_modules'
  )
  await okFile(path.join(pkgVariation1, '@pnpm.e2e/abc'))
  await okFile(path.join(pkgVariation1, '@pnpm.e2e/peer-a'))
  await okFile(path.join(pkgVariation1, '@pnpm.e2e/peer-b'))
  await okFile(path.join(pkgVariation1, '@pnpm.e2e/peer-c'))
  await okFile(path.join(pkgVariation1, '@pnpm.e2e/dep-of-pkg-with-1-dep'))

  const pkgVariation2 = path.join(
    'node_modules/.pnpm',
    depPathToFilename(`/@pnpm.e2e/abc@1.0.0${createPeersDirSuffix([{ name: '@pnpm.e2e/peer-a', version: '1.0.0' }, { name: '@pnpm.e2e/peer-b', version: '1.0.0' }, { name: '@pnpm.e2e/peer-c', version: '1.0.0' }])}`),
    'node_modules'
  )
  await okFile(path.join(pkgVariation2, '@pnpm.e2e/abc'))
  await okFile(path.join(pkgVariation2, '@pnpm.e2e/peer-a'))
  await okFile(path.join(pkgVariation2, '@pnpm.e2e/peer-b'))
  await okFile(path.join(pkgVariation2, '@pnpm.e2e/peer-c'))
  await okFile(path.join(pkgVariation2, '@pnpm.e2e/dep-of-pkg-with-1-dep'))

  expect(deepRequireCwd(['@pnpm.e2e/abc-parent-with-ab', '@pnpm.e2e/abc', '@pnpm.e2e/peer-c', './package.json']).version).toBe('2.0.0')
  expect(deepRequireCwd(['@pnpm.e2e/abc-grand-parent-with-c', '@pnpm.e2e/abc-parent-with-ab', '@pnpm.e2e/abc', '@pnpm.e2e/peer-c', './package.json']).version).toBe('1.0.0')

  // this part was failing. See issue: https://github.com/pnpm/pnpm/issues/1201
  await addDistTag({ package: '@pnpm.e2e/peer-a', version: '1.0.1', distTag: 'latest' })
  await install(manifest, await testDefaults({ autoInstallPeers: false, update: true, depth: 100, strictPeerDependencies: false }))
})

test('peer dependencies are linked when running two separate named installations', async () => {
  await addDistTag({ package: '@pnpm.e2e/abc-parent-with-ab', version: '1.0.0', distTag: 'latest' })
  await addDistTag({ package: '@pnpm.e2e/peer-a', version: '1.0.0', distTag: 'latest' })
  await addDistTag({ package: '@pnpm.e2e/peer-c', version: '1.0.0', distTag: 'latest' })
  prepareEmpty()

  const manifest = await addDependenciesToPackage({}, ['@pnpm.e2e/abc-grand-parent-with-c', '@pnpm.e2e/peer-c@2.0.0'], await testDefaults({ strictPeerDependencies: false }))
  await addDependenciesToPackage(manifest, ['@pnpm.e2e/abc-parent-with-ab'], await testDefaults({ strictPeerDependencies: false }))

  const pkgVariation1 = path.join(
    'node_modules/.pnpm',
    depPathToFilename(`/@pnpm.e2e/abc@1.0.0${createPeersDirSuffix([{ name: '@pnpm.e2e/peer-a', version: '1.0.0' }, { name: '@pnpm.e2e/peer-b', version: '1.0.0' }, { name: '@pnpm.e2e/peer-c', version: '1.0.0' }])}`),
    'node_modules'
  )
  await okFile(path.join(pkgVariation1, '@pnpm.e2e/abc'))
  await okFile(path.join(pkgVariation1, '@pnpm.e2e/peer-a'))
  await okFile(path.join(pkgVariation1, '@pnpm.e2e/peer-b'))
  await okFile(path.join(pkgVariation1, '@pnpm.e2e/peer-c'))
  await okFile(path.join(pkgVariation1, '@pnpm.e2e/dep-of-pkg-with-1-dep'))

  const pkgVariation2 = path.join(
    'node_modules/.pnpm',
    depPathToFilename(`/@pnpm.e2e/abc@1.0.0${createPeersDirSuffix([{ name: '@pnpm.e2e/peer-a', version: '1.0.0' }, { name: '@pnpm.e2e/peer-b', version: '1.0.0' }, { name: '@pnpm.e2e/peer-c', version: '2.0.0' }])}`),
    'node_modules'
  )
  await okFile(path.join(pkgVariation2, '@pnpm.e2e/abc'))
  await okFile(path.join(pkgVariation2, '@pnpm.e2e/peer-a'))
  await okFile(path.join(pkgVariation2, '@pnpm.e2e/peer-b'))
  await okFile(path.join(pkgVariation2, '@pnpm.e2e/dep-of-pkg-with-1-dep'))

  expect(deepRequireCwd(['@pnpm.e2e/abc-parent-with-ab', '@pnpm.e2e/abc', '@pnpm.e2e/peer-c', './package.json']).version).toBe('2.0.0')
  expect(deepRequireCwd(['@pnpm.e2e/abc-grand-parent-with-c', '@pnpm.e2e/abc-parent-with-ab', '@pnpm.e2e/abc', '@pnpm.e2e/peer-c', './package.json']).version).toBe('1.0.0')
})

test.skip('peer dependencies are linked', async () => {
  const project = prepareEmpty()
  await install({
    dependencies: {
      '@pnpm.e2e/abc-grand-parent-with-c': '*',
      '@pnpm.e2e/peer-c': '2.0.0',
    },
    devDependencies: {
      '@pnpm.e2e/abc-parent-with-ab': '*',
    },
  }, await testDefaults())

  const pkgVariationsDir = path.resolve('node_modules/.pnpm/abc@1.0.0')

  const pkgVariation1 = path.join(pkgVariationsDir, '165e1e08a3f7e7f77ddb572ad0e55660/node_modules')
  await okFile(path.join(pkgVariation1, '@pnpm.e2e/abc'))
  await okFile(path.join(pkgVariation1, '@pnpm.e2e/peer-a'))
  await okFile(path.join(pkgVariation1, '@pnpm.e2e/peer-b'))
  await okFile(path.join(pkgVariation1, '@pnpm.e2e/peer-c'))
  await okFile(path.join(pkgVariation1, '@pnpm.e2e/dep-of-pkg-with-1-dep'))

  const pkgVariation2 = path.join(pkgVariationsDir, '@pnpm.e2e+peer-a@1.0.0+@pnpm.e2e+peer-b@1.0.0/node_modules')
  await okFile(path.join(pkgVariation2, '@pnpm.e2e/abc'))
  await okFile(path.join(pkgVariation2, '@pnpm.e2e/peer-a'))
  await okFile(path.join(pkgVariation2, '@pnpm.e2e/peer-b'))
  await okFile(path.join(pkgVariation2, '@pnpm.e2e/dep-of-pkg-with-1-dep'))

  expect(deepRequireCwd(['@pnpm.e2e/abc-parent-with-ab', '@pnpm.e2e/abc', '@pnpm.e2e/peer-c', './package.json']).version).toBe('2.0.0')
  expect(deepRequireCwd(['@pnpm.e2e/abc-grand-parent-with-c', '@pnpm.e2e/abc-parent-with-ab', '@pnpm.e2e/abc', '@pnpm.e2e/peer-c', './package.json']).version).toBe('1.0.0')

  const lockfile = project.readLockfile()
  expect(lockfile.packages['/@pnpm.e2e/abc-parent-with-ab/1.0.0/@pnpm.e2e/peer-a@1.0.0+@pnpm.e2e+peer-b@1.0.0'].dev).toBeTruthy()
})

test('scoped peer dependency is linked', async () => {
  prepareEmpty()
  await addDependenciesToPackage({}, ['@pnpm.e2e/for-testing-scoped-peers'], await testDefaults())

  const pkgVariation = path.resolve('node_modules/.pnpm/@having+scoped-peer@1.0.0_@scoped+peer@1.0.0/node_modules')
  await okFile(path.join(pkgVariation, '@having', 'scoped-peer'))
  await okFile(path.join(pkgVariation, '@scoped', 'peer'))
})

test('peer bins are linked', async () => {
  const project = prepareEmpty()

  await addDependenciesToPackage({}, ['@pnpm.e2e/for-testing-peers-having-bins'], await testDefaults({ fastUnpack: false }))

  const suffix = createPeersDirSuffix([{ name: '@pnpm.e2e/peer-with-bin', version: '1.0.0' }])
  const pkgVariation = path.join('.pnpm', depPathToFilename(`/@pnpm.e2e/pkg-with-peer-having-bin@1.0.0${suffix}`), 'node_modules')

  project.isExecutable(path.join(pkgVariation, '@pnpm.e2e/pkg-with-peer-having-bin/node_modules/.bin', 'peer-with-bin'))

  project.isExecutable(path.join(pkgVariation, '@pnpm.e2e/pkg-with-peer-having-bin/node_modules/.bin', 'hello-world-js-bin'))
})

test('run pre/postinstall scripts of each variations of packages with peer dependencies', async () => {
  await addDistTag({ package: '@pnpm.e2e/peer-c', version: '1.0.0', distTag: 'latest' })
  prepareEmpty()
  await addDependenciesToPackage({}, ['@pnpm.e2e/parent-of-pkg-with-events-and-peers', '@pnpm.e2e/pkg-with-events-and-peers', '@pnpm.e2e/peer-c@2.0.0'], await testDefaults({ fastUnpack: false }))

  const pkgVariation1 = path.resolve('node_modules/.pnpm/@pnpm.e2e+pkg-with-events-and-peers@1.0.0_@pnpm.e2e+peer-c@1.0.0/node_modules')
  await okFile(path.join(pkgVariation1, '@pnpm.e2e/pkg-with-events-and-peers', 'generated-by-preinstall.js'))
  await okFile(path.join(pkgVariation1, '@pnpm.e2e/pkg-with-events-and-peers', 'generated-by-postinstall.js'))

  const pkgVariation2 = path.resolve('node_modules/.pnpm/@pnpm.e2e+pkg-with-events-and-peers@1.0.0_@pnpm.e2e+peer-c@2.0.0/node_modules')
  await okFile(path.join(pkgVariation2, '@pnpm.e2e/pkg-with-events-and-peers', 'generated-by-preinstall.js'))
  await okFile(path.join(pkgVariation2, '@pnpm.e2e/pkg-with-events-and-peers', 'generated-by-postinstall.js'))
})

test('package that has parent as peer dependency', async () => {
  const project = prepareEmpty()
  await addDependenciesToPackage({}, ['@pnpm.e2e/has-alpha', '@pnpm.e2e/alpha'], await testDefaults())

  const lockfile = project.readLockfile()

  expect(lockfile.packages).toHaveProperty(['/@pnpm.e2e/has-alpha-as-peer@1.0.0(@pnpm.e2e/alpha@1.0.0)'])
  expect(lockfile.packages).not.toHaveProperty(['/@pnpm.e2e/has-alpha-as-peer@1.0.0'])
})

test('own peer installed in root as well is linked to root', async () => {
  prepareEmpty()

  await addDependenciesToPackage({}, ['is-negative@kevva/is-negative#2.1.0', '@pnpm.e2e/peer-deps-in-child-pkg'], await testDefaults())

  expect(deepRequireCwd.silent(['is-negative', './package.json'])).toBeTruthy()
})

test('peer dependency is grouped with dependent when the peer is a top dependency but an external lockfile is used', async () => {
  prepareEmpty()

  const reporter = sinon.spy()

  await addDependenciesToPackage({}, ['ajv@4.10.4', 'ajv-keywords@1.5.0'], await testDefaults({ reporter, lockfileDir: path.resolve('..'), strictPeerDependencies: false }))

  expect(reporter.calledWithMatch({
    message: `localhost+${REGISTRY_MOCK_PORT}/ajv-keywords@1.5.0 requires a peer of ajv@>=4.10.0 but none was installed.`,
  })).toBeFalsy()

  expect(fs.existsSync(path.join('../node_modules/.pnpm/ajv-keywords@1.5.0_ajv@4.10.4/node_modules/ajv-keywords'))).toBeTruthy()

  const lockfile = readYamlFile<Lockfile>(path.join('..', WANTED_LOCKFILE))

  expect(lockfile.importers.project).toStrictEqual({
    dependencies: {
      ajv: {
        specifier: '4.10.4',
        version: '4.10.4',
      },
      'ajv-keywords': {
        specifier: '1.5.0',
        version: '1.5.0(ajv@4.10.4)',
      },
    },
  })
})

// Covers https://github.com/pnpm/pnpm/issues/1483
test('peer dependency is grouped correctly with peer installed via separate installation when external lockfile is used', async () => {
  prepareEmpty()

  const reporter = sinon.spy()
  const lockfileDir = path.resolve('..')

  const manifest = await install({
    dependencies: {
      '@pnpm.e2e/abc': '1.0.0',
    },
  }, await testDefaults({ autoInstallPeers: false, reporter, lockfileDir, strictPeerDependencies: false }))
  await addDependenciesToPackage(
    manifest,
    ['@pnpm.e2e/peer-c@2.0.0'],
    await testDefaults({ autoInstallPeers: false, reporter, lockfileDir, strictPeerDependencies: false })
  )

  expect(fs.existsSync(path.join('../node_modules/.pnpm/@pnpm.e2e+abc@1.0.0_@pnpm.e2e+peer-c@2.0.0/node_modules/@pnpm.e2e/dep-of-pkg-with-1-dep'))).toBeTruthy()
})

test('peer dependency is grouped with dependent when the peer is a top dependency and external node_modules is used', async () => {
  prepareEmpty()
  fs.mkdirSync('_')
  process.chdir('_')
  const lockfileDir = path.resolve('..')

  let manifest = await addDependenciesToPackage({}, ['ajv@4.10.4', 'ajv-keywords@1.5.0'], await testDefaults({ lockfileDir, strictPeerDependencies: false }))

  {
    const lockfile = readYamlFile<Lockfile>(path.resolve('..', WANTED_LOCKFILE))
    expect(lockfile.importers._).toStrictEqual({
      dependencies: {
        ajv: {
          specifier: '4.10.4',
          version: '4.10.4',
        },
        'ajv-keywords': {
          specifier: '1.5.0',
          version: '1.5.0(ajv@4.10.4)',
        },
      },
    })
  }

  manifest = await install(manifest, await testDefaults({ lockfileDir, strictPeerDependencies: false }))

  {
    const lockfile = readYamlFile<Lockfile>(path.resolve('..', WANTED_LOCKFILE))
    expect(lockfile.importers._).toStrictEqual({
      dependencies: {
        ajv: {
          specifier: '4.10.4',
          version: '4.10.4',
        },
        'ajv-keywords': {
          specifier: '1.5.0',
          version: '1.5.0(ajv@4.10.4)',
        },
      },
    })
  }

  // Covers https://github.com/pnpm/pnpm/issues/1506
  await mutateModulesInSingleProject({
    dependencyNames: ['ajv'],
    manifest,
    mutation: 'uninstallSome',
    rootDir: process.cwd(),
  }, await testDefaults({
    autoInstallPeers: false,
    lockfileDir,
    strictPeerDependencies: false,
  })
  )

  {
    const lockfile = readYamlFile<Lockfile>(path.resolve('..', WANTED_LOCKFILE))
    expect(lockfile.importers._).toStrictEqual({
      dependencies: {
        'ajv-keywords': {
          specifier: '1.5.0',
          version: '1.5.0',
        },
      },
    })
  }
})

test('external lockfile: peer dependency is grouped with dependent even after a named update', async () => {
  prepareEmpty()
  fs.mkdirSync('_')
  process.chdir('_')
  const lockfileDir = path.resolve('..')

  const manifest = await addDependenciesToPackage({}, ['ajv@4.10.4', 'ajv-keywords@1.4.0'], await testDefaults({ lockfileDir }))

  {
    const lockfile = readYamlFile<Lockfile>(path.resolve('..', WANTED_LOCKFILE))
    expect(lockfile.importers._).toStrictEqual({
      dependencies: {
        ajv: {
          specifier: '4.10.4',
          version: '4.10.4',
        },
        'ajv-keywords': {
          specifier: '1.4.0',
          version: '1.4.0(ajv@4.10.4)',
        },
      },
    })
  }

  await addDependenciesToPackage(manifest, ['ajv-keywords@1.5.0'], await testDefaults({ lockfileDir }))

  {
    const lockfile = readYamlFile<Lockfile>(path.resolve('..', WANTED_LOCKFILE))
    expect(lockfile.importers._).toStrictEqual({
      dependencies: {
        ajv: {
          specifier: '4.10.4',
          version: '4.10.4',
        },
        'ajv-keywords': {
          specifier: '1.5.0',
          version: '1.5.0(ajv@4.10.4)',
        },
      },
    })
  }
})

test('external lockfile: peer dependency is grouped with dependent even after a named update of the resolved package', async () => {
  prepareEmpty()
  fs.mkdirSync('_')
  process.chdir('_')
  const lockfileDir = path.resolve('..')

  const manifest = await addDependenciesToPackage({}, ['@pnpm.e2e/peer-c@1.0.0', '@pnpm.e2e/abc-parent-with-ab@1.0.0'], await testDefaults({ lockfileDir, strictPeerDependencies: false }))

  {
    const lockfile = readYamlFile<Lockfile>(path.resolve('..', WANTED_LOCKFILE))
    expect(lockfile.importers._).toStrictEqual({
      dependencies: {
        '@pnpm.e2e/abc-parent-with-ab': {
          specifier: '1.0.0',
          version: '1.0.0(@pnpm.e2e/peer-c@1.0.0)',
        },
        '@pnpm.e2e/peer-c': {
          specifier: '1.0.0',
          version: '1.0.0',
        },
      },
    })
  }

  await addDependenciesToPackage(manifest, ['@pnpm.e2e/peer-c@2.0.0'], await testDefaults({ lockfileDir, strictPeerDependencies: false }))

  {
    const lockfile = readYamlFile<Lockfile>(path.resolve('..', WANTED_LOCKFILE))
    expect(lockfile.importers._).toStrictEqual({
      dependencies: {
        '@pnpm.e2e/abc-parent-with-ab': {
          specifier: '1.0.0',
          version: '1.0.0(@pnpm.e2e/peer-c@2.0.0)',
        },
        '@pnpm.e2e/peer-c': {
          specifier: '2.0.0',
          version: '2.0.0',
        },
      },
    })
  }

  expect(fs.existsSync(path.join('../node_modules/.pnpm/@pnpm.e2e+abc-parent-with-ab@1.0.0_@pnpm.e2e+peer-c@2.0.0/node_modules/is-positive'))).toBeTruthy()
})

test('regular dependencies are not removed on update from transitive packages that have children with peers resolved from above', async () => {
  prepareEmpty()
  fs.mkdirSync('_')
  process.chdir('_')
  const lockfileDir = path.resolve('..')
  await addDistTag({ package: '@pnpm.e2e/abc-parent-with-ab', version: '1.0.1', distTag: 'latest' })
  await addDistTag({ package: '@pnpm.e2e/peer-c', version: '1.0.0', distTag: 'latest' })

  const manifest = await addDependenciesToPackage({}, ['@pnpm.e2e/abc-grand-parent-with-c@1.0.0'], await testDefaults({ lockfileDir }))

  await addDistTag({ package: '@pnpm.e2e/peer-c', version: '1.0.1', distTag: 'latest' })
  await install(manifest, await testDefaults({ lockfileDir, update: true, depth: 2 }))

  expect(fs.existsSync(path.join('../node_modules/.pnpm/@pnpm.e2e+abc-parent-with-ab@1.0.1_@pnpm.e2e+peer-c@1.0.1/node_modules/is-positive'))).toBeTruthy()
})

test('peer dependency is resolved from parent package', async () => {
  preparePackages([
    {
      name: 'pkg',
    },
  ])
  await mutateModulesInSingleProject({
    dependencySelectors: ['@pnpm.e2e/tango@1.0.0'],
    manifest: {},
    mutation: 'installSome',
    rootDir: path.resolve('pkg'),
  }, await testDefaults())

  const lockfile = readYamlFile<Lockfile>(WANTED_LOCKFILE)
  expect(Object.keys(lockfile.packages ?? {})).toStrictEqual([
    '/@pnpm.e2e/has-tango-as-peer-dep@1.0.0(@pnpm.e2e/tango@1.0.0)',
    '/@pnpm.e2e/tango@1.0.0',
  ])
})

test('transitive peerDependencies field does not break the lockfile on subsequent named install', async () => {
  preparePackages([
    {
      name: 'pkg',
    },
  ])
  const { manifest } = await mutateModulesInSingleProject({
    dependencySelectors: ['most@1.7.3'],
    manifest: {},
    mutation: 'installSome',
    rootDir: path.resolve('pkg'),
  }, await testDefaults())

  await mutateModulesInSingleProject({
    dependencySelectors: ['is-positive'],
    manifest,
    mutation: 'installSome',
    rootDir: path.resolve('pkg'),
  }, await testDefaults())

  const lockfile = readYamlFile<Lockfile>(WANTED_LOCKFILE)

  expect(Object.keys(lockfile.packages!['/most@1.7.3'].dependencies!)).toStrictEqual([
    '@most/multicast',
    '@most/prelude',
    'symbol-observable',
  ])
})

test('peer dependency is resolved from parent package via its alias', async () => {
  preparePackages([
    {
      name: 'pkg',
    },
  ])
  await mutateModulesInSingleProject({
    dependencySelectors: ['@pnpm.e2e/tango@npm:@pnpm.e2e/tango-tango@1.0.0'],
    manifest: {},
    mutation: 'installSome',
    rootDir: path.resolve('pkg'),
  }, await testDefaults())

  const lockfile = readYamlFile<Lockfile>(WANTED_LOCKFILE)
  expect(Object.keys(lockfile.packages ?? {})).toStrictEqual([
    '/@pnpm.e2e/has-tango-as-peer-dep@1.0.0(@pnpm.e2e/tango-tango@1.0.0(@pnpm.e2e/tango-tango@1.0.0))',
    '/@pnpm.e2e/tango-tango@1.0.0(@pnpm.e2e/tango-tango@1.0.0)',
  ])
})

test('peer dependency is saved', async () => {
  prepareEmpty()

  const manifest = await addDependenciesToPackage(
    {},
    ['is-positive@1.0.0'],
    await testDefaults({
      peer: true,
      targetDependenciesField: 'devDependencies',
    })
  )

  expect(manifest).toStrictEqual(
    {
      devDependencies: { 'is-positive': '1.0.0' },
      peerDependencies: { 'is-positive': '1.0.0' },
    }
  )

  const mutatedImporter = await mutateModulesInSingleProject({
    dependencyNames: ['is-positive'],
    manifest,
    mutation: 'uninstallSome',
    rootDir: process.cwd(),
  }, await testDefaults())

  expect(mutatedImporter.manifest).toStrictEqual(
    {
      devDependencies: {},
      peerDependencies: {},
    }
  )
})

test('warning is not reported when cannot resolve optional peer dependency', async () => {
  const project = prepareEmpty()

  const reporter = jest.fn()

  await addDependenciesToPackage(
    {},
    ['@pnpm.e2e/abc-optional-peers@1.0.0', '@pnpm.e2e/peer-c@2.0.0'],
    await testDefaults({ autoInstallPeers: false, reporter, strictPeerDependencies: false })
  )

  expect(reporter).toHaveBeenCalledWith(
    expect.objectContaining({
      level: 'debug',
      name: 'pnpm:peer-dependency-issues',
      issuesByProjects: {
        '.': {
          bad: {
            '@pnpm.e2e/peer-c': [{
              parents: [
                {
                  name: '@pnpm.e2e/abc-optional-peers',
                  version: '1.0.0',
                },
              ],
              foundVersion: '2.0.0',
              resolvedFrom: [],
              optional: true,
              wantedRange: '^1.0.0',
            }],
          },
          missing: {
            '@pnpm.e2e/peer-a': [
              {
                parents: [
                  {
                    name: '@pnpm.e2e/abc-optional-peers',
                    version: '1.0.0',
                  },
                ],
                optional: false,
                wantedRange: '^1.0.0',
              },
            ],
            '@pnpm.e2e/peer-b': [
              {
                parents: [
                  {
                    name: '@pnpm.e2e/abc-optional-peers',
                    version: '1.0.0',
                  },
                ],
                optional: true,
                wantedRange: '^1.0.0',
              },
            ],
          },
          conflicts: [],
          intersections: { '@pnpm.e2e/peer-a': '^1.0.0' },
        },
      },
    })
  )

  const lockfile = project.readLockfile()

  expect(lockfile.packages['/@pnpm.e2e/abc-optional-peers@1.0.0(@pnpm.e2e/peer-c@2.0.0)'].peerDependenciesMeta).toStrictEqual({
    '@pnpm.e2e/peer-b': {
      optional: true,
    },
    '@pnpm.e2e/peer-c': {
      optional: true,
    },
  })
})

test('warning is not reported when cannot resolve optional peer dependency (specified by meta field only)', async () => {
  const project = prepareEmpty()

  const reporter = jest.fn()

  await addDependenciesToPackage(
    {},
    ['@pnpm.e2e/abc-optional-peers-meta-only@1.0.0', '@pnpm.e2e/peer-c@2.0.0'],
    await testDefaults({ autoInstallPeers: false, reporter, strictPeerDependencies: false })
  )

  expect(reporter).toHaveBeenCalledWith(
    expect.objectContaining({
      level: 'debug',
      name: 'pnpm:peer-dependency-issues',
      issuesByProjects: {
        '.': {
          bad: {},
          missing: {
            '@pnpm.e2e/peer-a': [
              {
                parents: [
                  {
                    name: '@pnpm.e2e/abc-optional-peers-meta-only',
                    version: '1.0.0',
                  },
                ],
                optional: false,
                wantedRange: '^1.0.0',
              },
            ],
            '@pnpm.e2e/peer-b': [
              {
                parents: [
                  {
                    name: '@pnpm.e2e/abc-optional-peers-meta-only',
                    version: '1.0.0',
                  },
                ],
                optional: true,
                wantedRange: '*',
              },
            ],
          },
          conflicts: [],
          intersections: { '@pnpm.e2e/peer-a': '^1.0.0' },
        },
      },
    })
  )

  const lockfile = project.readLockfile()

  expect(lockfile.packages['/@pnpm.e2e/abc-optional-peers-meta-only@1.0.0(@pnpm.e2e/peer-c@2.0.0)'].peerDependencies).toStrictEqual({
    '@pnpm.e2e/peer-a': '^1.0.0',
    '@pnpm.e2e/peer-b': '*',
    '@pnpm.e2e/peer-c': '*',
  })
  expect(lockfile.packages['/@pnpm.e2e/abc-optional-peers-meta-only@1.0.0(@pnpm.e2e/peer-c@2.0.0)'].peerDependenciesMeta).toStrictEqual({
    '@pnpm.e2e/peer-b': {
      optional: true,
    },
    '@pnpm.e2e/peer-c': {
      optional: true,
    },
  })
})

test('local tarball dependency with peer dependency', async () => {
  prepareEmpty()

  const reporter = sinon.spy()

  const manifest = await addDependenciesToPackage({}, [
    `file:${f.find('tar-pkg-with-peers/tar-pkg-with-peers-1.0.0.tgz')}`,
    'bar@npm:@pnpm.e2e/bar@100.0.0',
    'foo@npm:@pnpm.e2e/foo@100.0.0',
  ], await testDefaults({ reporter }))

  const integrityLocalPkgDirs = fs.readdirSync('node_modules/.pnpm')
    .filter((dir) => dir.startsWith('file+'))

  expect(integrityLocalPkgDirs.length).toBe(1)

  rimraf('node_modules')

  await mutateModulesInSingleProject({
    manifest,
    mutation: 'install',
    rootDir: process.cwd(),
  }, await testDefaults())

  {
    const updatedLocalPkgDirs = fs.readdirSync('node_modules/.pnpm')
      .filter((dir) => dir.startsWith('file+'))
    expect(updatedLocalPkgDirs).toStrictEqual(integrityLocalPkgDirs)
  }
})

test('peer dependency that is resolved by a dev dependency', async () => {
  const project = prepareEmpty()
  const manifest = {
    dependencies: {
      '@typegoose/typegoose': '7.3.0',
    },
    devDependencies: {
      '@types/mongoose': '5.7.32',
    },
  }

  await mutateModulesInSingleProject({
    manifest,
    mutation: 'install',
    rootDir: process.cwd(),
  }, await testDefaults({ fastUnpack: false, lockfileOnly: true, strictPeerDependencies: false }))

  const lockfile = project.readLockfile()
  expect(lockfile.packages['/@types/mongoose@5.7.32'].dev).toBeUndefined()

  await mutateModulesInSingleProject({
    manifest,
    mutation: 'install',
    rootDir: process.cwd(),
  }, await testDefaults({
    frozenLockfile: true,
    include: {
      dependencies: true,
      devDependencies: false,
      optionalDependencies: false,
    },
  }))

  project.has('@typegoose/typegoose')
  project.hasNot('@types/mongoose')
})

test('peer dependency is grouped with dependency when peer is resolved not from a top dependency 2', async () => {
  const project1Manifest = {
    name: 'project-1',
    version: '1.0.0',
    dependencies: {
      'ajv-keywords': '1.5.0',
      ajv: 'link:../ajv',
    },
  }
  const project2Manifest = {
    name: 'ajv',
    version: '4.10.4',
  }
  preparePackages([
    {
      location: 'project-1',
      package: project1Manifest,
    },
    {
      location: 'ajv',
      package: project2Manifest,
    },
  ])
  const importers: MutatedProject[] = [
    {
      mutation: 'install',
      rootDir: path.resolve('project-1'),
    },
    {
      mutation: 'install',
      rootDir: path.resolve('ajv'),
    },
  ]
  const allProjects = [
    {
      buildIndex: 0,
      manifest: project1Manifest,
      rootDir: path.resolve('project-1'),
    },
    {
      buildIndex: 0,
      manifest: project2Manifest,
      rootDir: path.resolve('ajv'),
    },
  ]
  await mutateModules(importers, await testDefaults({ allProjects }))

  const lockfile = readYamlFile<Lockfile>(path.resolve(WANTED_LOCKFILE))
  expect(lockfile.packages?.['/ajv-keywords@1.5.0(ajv@ajv)'].dependencies?.['ajv']).toBe('link:ajv')
})

test('deduplicate packages that have optional and non-optional peers', async () => {
  await addDistTag({ package: '@pnpm.e2e/peer-a', version: '1.0.0', distTag: 'latest' })
  await addDistTag({ package: '@pnpm.e2e/peer-b', version: '1.0.0', distTag: 'latest' })
  await addDistTag({ package: '@pnpm.e2e/peer-c', version: '1.0.0', distTag: 'latest' })
  prepareEmpty()
  await addDependenciesToPackage({},
    ['@pnpm.e2e/abc-optional-peers', '@pnpm.e2e/abc-optional-peers-parent'],
    await testDefaults({ autoInstallPeers: false, dedupePeerDependents: true })
  )

  const lockfile = readYamlFile<Lockfile>(path.resolve(WANTED_LOCKFILE))
  const depPaths = Object.keys(lockfile.packages ?? {})
  expect(depPaths.length).toBe(5)
  expect(depPaths).toContain(`/@pnpm.e2e/abc-optional-peers@1.0.0${createPeersDirSuffix([{ name: '@pnpm.e2e/peer-a', version: '1.0.0' }, { name: '@pnpm.e2e/peer-b', version: '1.0.0' }, { name: '@pnpm.e2e/peer-c', version: '1.0.0' }])}`)
})

test('deduplicate packages that have peers', async () => {
  await addDistTag({ package: '@pnpm.e2e/abc-parent-with-ab', version: '1.0.0', distTag: 'latest' })
  await addDistTag({ package: '@pnpm.e2e/peer-a', version: '1.0.0', distTag: 'latest' })
  await addDistTag({ package: '@pnpm.e2e/peer-b', version: '1.0.0', distTag: 'latest' })
  await addDistTag({ package: '@pnpm.e2e/peer-c', version: '1.0.0', distTag: 'latest' })

  prepareEmpty()
  await addDependenciesToPackage({},
    ['@pnpm.e2e/abc-grand-parent-with-c@1.0.0', '@pnpm.e2e/abc-parent-with-ab@1.0.0', '@pnpm.e2e/abc@1.0.0'],
    await testDefaults({ autoInstallPeers: false, dedupePeerDependents: true })
  )

  const lockfile = readYamlFile<Lockfile>(path.resolve(WANTED_LOCKFILE))
  const depPaths = Object.keys(lockfile.packages ?? {})
  expect(depPaths.length).toBe(8)
  expect(depPaths).toContain(`/@pnpm.e2e/abc@1.0.0${createPeersDirSuffix([{ name: '@pnpm.e2e/peer-a', version: '1.0.0' }, { name: '@pnpm.e2e/peer-b', version: '1.0.0' }, { name: '@pnpm.e2e/peer-c', version: '1.0.0' }])}`)
  expect(depPaths).toContain(`/@pnpm.e2e/abc-parent-with-ab@1.0.0${createPeersDirSuffix([{ name: '@pnpm.e2e/peer-c', version: '1.0.0' }])}`)
})

test('deduplicate packages that have peers, when adding new dependency in a workspace', async () => {
  await addDistTag({ package: '@pnpm.e2e/abc-parent-with-ab', version: '1.0.0', distTag: 'latest' })
  await addDistTag({ package: '@pnpm.e2e/peer-a', version: '1.0.0', distTag: 'latest' })
  await addDistTag({ package: '@pnpm.e2e/peer-b', version: '1.0.0', distTag: 'latest' })
  await addDistTag({ package: '@pnpm.e2e/peer-c', version: '1.0.0', distTag: 'latest' })

  const manifest1 = {
    name: 'project-1',

    dependencies: {
      '@pnpm.e2e/abc-grand-parent-with-c': '1.0.0',
    },
  }
  const manifest2 = {
    name: 'project-2',
  }
  preparePackages([
    {
      location: 'project-1',
      package: manifest1,
    },
    {
      location: 'project-2',
      package: manifest2,
    },
  ])

  const importers: MutatedProject[] = [
    {
      mutation: 'install',
      rootDir: path.resolve('project-1'),
    },
    {
      mutation: 'install',
      rootDir: path.resolve('project-2'),
    },
  ]
  const allProjects = [
    {
      buildIndex: 0,
      manifest: manifest1,
      rootDir: path.resolve('project-1'),
    },
    {
      buildIndex: 0,
      manifest: manifest2,
      rootDir: path.resolve('project-2'),
    },
  ]
  await mutateModules(importers, await testDefaults({ allProjects, autoInstallPeers: false, dedupePeerDependents: true }))
  importers[1] = {
    dependencySelectors: ['@pnpm.e2e/abc@1.0.0'],
    mutation: 'installSome',
    rootDir: path.resolve('project-2'),
  }
  await mutateModules(importers, await testDefaults({ allProjects, autoInstallPeers: false, dedupePeerDependents: true }))

  const lockfile = readYamlFile<Lockfile>(path.resolve(WANTED_LOCKFILE))
  const depPaths = Object.keys(lockfile.packages ?? {})
  expect(depPaths.length).toBe(8)
  expect(depPaths).toContain(`/@pnpm.e2e/abc@1.0.0${createPeersDirSuffix([{ name: '@pnpm.e2e/peer-a', version: '1.0.0' }, { name: '@pnpm.e2e/peer-b', version: '1.0.0' }, { name: '@pnpm.e2e/peer-c', version: '1.0.0' }])}`)
  expect(depPaths).toContain(`/@pnpm.e2e/abc-parent-with-ab@1.0.0${createPeersDirSuffix([{ name: '@pnpm.e2e/peer-c', version: '1.0.0' }])}`)
})

test('resolve peer dependencies from aliased subdependencies if they are dependencies of a parent package', async () => {
  prepareEmpty()
  await addDistTag({ package: '@pnpm.e2e/peer-a', version: '1.0.0', distTag: 'latest' })
  await addDistTag({ package: '@pnpm.e2e/peer-c', version: '1.0.0', distTag: 'latest' })

  await addDependenciesToPackage(
    {},
    ['@pnpm.e2e/abc-parent-with-aliases'],
    await testDefaults({ autoInstallPeers: false, strictPeerDependencies: false })
  )

  const lockfile = readYamlFile<any>(path.resolve(WANTED_LOCKFILE)) // eslint-disable-line
  expect(lockfile.packages['/@pnpm.e2e/abc@1.0.0(@pnpm.e2e/peer-a@1.0.0)(@pnpm.e2e/peer-b@1.0.0)(@pnpm.e2e/peer-c@1.0.0)']).toBeTruthy()
})

test('resolve peer dependency from aliased direct dependency', async () => {
  prepareEmpty()

  const opts = await testDefaults({ autoInstallPeers: false, strictPeerDependencies: false })
  const manifest = await addDependenciesToPackage({}, ['peer-a@npm:@pnpm.e2e/peer-a@1.0.0'], opts)
  await addDependenciesToPackage(manifest, ['@pnpm.e2e/abc@1.0.0'], opts)

  const lockfile = readYamlFile<any>(path.resolve(WANTED_LOCKFILE)) // eslint-disable-line
  expect(lockfile.packages['/@pnpm.e2e/abc@1.0.0(@pnpm.e2e/peer-a@1.0.0)']).toBeTruthy()
})

test('resolve peer dependency using the alias that differs from the real name of the direct dependency', async () => {
  prepareEmpty()

  const opts = await testDefaults({ autoInstallPeers: false, strictPeerDependencies: false })
  const manifest = await addDependenciesToPackage({}, ['@pnpm.e2e/peer-b@npm:@pnpm.e2e/peer-a@1.0.0'], opts)
  await addDependenciesToPackage(manifest, ['@pnpm.e2e/abc@1.0.0'], opts)

  const lockfile = readYamlFile<any>(path.resolve(WANTED_LOCKFILE)) // eslint-disable-line
  expect(lockfile.packages['/@pnpm.e2e/abc@1.0.0(@pnpm.e2e/peer-a@1.0.0)(@pnpm.e2e/peer-a@1.0.0)']).toBeTruthy()
  expect(lockfile.packages['/@pnpm.e2e/abc@1.0.0(@pnpm.e2e/peer-a@1.0.0)(@pnpm.e2e/peer-a@1.0.0)']?.dependencies['@pnpm.e2e/peer-a']).toBe('1.0.0')
  expect(lockfile.packages['/@pnpm.e2e/abc@1.0.0(@pnpm.e2e/peer-a@1.0.0)(@pnpm.e2e/peer-a@1.0.0)']?.dependencies['@pnpm.e2e/peer-b']).toBe('/@pnpm.e2e/peer-a@1.0.0')
})

test('when there are several aliased dependencies of the same package, pick the one with the highest version to resolve peers', async () => {
  prepareEmpty()

  const opts = await testDefaults({ autoInstallPeers: false, strictPeerDependencies: false })
  const manifest = await addDependenciesToPackage({}, [
    'peer-c3@npm:@pnpm.e2e/peer-c@1.0.0',
    'peer-c2@npm:@pnpm.e2e/peer-c@1.0.1',
    'peer-c1@npm:@pnpm.e2e/peer-c@2.0.0',
  ], opts)
  await addDependenciesToPackage(manifest, ['@pnpm.e2e/abc@1.0.0'], opts)

  const lockfile = readYamlFile<any>(path.resolve(WANTED_LOCKFILE)) // eslint-disable-line
  expect(lockfile.packages['/@pnpm.e2e/abc@1.0.0(@pnpm.e2e/peer-c@2.0.0)']).toBeTruthy()
})

test('when there is an aliases dependency and a non-aliased one, prefer the non-aliased dependency to resolve peers', async () => {
  prepareEmpty()

  const opts = await testDefaults({ autoInstallPeers: false, strictPeerDependencies: false })
  const manifest = await addDependenciesToPackage({}, [
    '@pnpm.e2e/peer-c@1.0.0',
    'peer-c@npm:@pnpm.e2e/peer-c@2.0.0',
  ], opts)
  await addDependenciesToPackage(manifest, ['@pnpm.e2e/abc@1.0.0'], opts)

  const lockfile = readYamlFile<any>(path.resolve(WANTED_LOCKFILE)) // eslint-disable-line
  expect(lockfile.packages['/@pnpm.e2e/abc@1.0.0(@pnpm.e2e/peer-c@1.0.0)']).toBeTruthy()
})

test('in a subdependency, when there are several aliased dependencies of the same package, pick the one with the highest version to resolve peers', async () => {
  prepareEmpty()

  await addDependenciesToPackage({}, ['@pnpm.e2e/abc-parent-with-aliases-of-same-pkg@1.0.0'], await testDefaults({ autoInstallPeers: false, strictPeerDependencies: false }))

  const lockfile = readYamlFile<any>(path.resolve(WANTED_LOCKFILE)) // eslint-disable-line
  expect(lockfile.packages['/@pnpm.e2e/abc@1.0.0(@pnpm.e2e/peer-c@2.0.0)']).toBeTruthy()
})

test('peer having peer is resolved correctly', async () => {
  const manifest1 = {
    name: 'project-1',

    dependencies: {
      '@pnpm.e2e/has-has-y-peer-only-as-peer': '1.0.0',
      '@pnpm.e2e/has-y-peer': '1.0.0',
      '@pnpm/y': '1.0.0',
    },
  }
  const manifest2 = {
    name: 'project-2',

    dependencies: {
      '@pnpm.e2e/has-has-y-peer-only-as-peer': '1.0.0',
      '@pnpm.e2e/has-y-peer': '1.0.0',
      '@pnpm/y': '2.0.0',
    },
  }
  preparePackages([
    {
      location: 'project-1',
      package: manifest1,
    },
    {
      location: 'project-2',
      package: manifest2,
    },
  ])

  const importers: MutatedProject[] = [
    {
      mutation: 'install',
      rootDir: path.resolve('project-1'),
    },
    {
      mutation: 'install',
      rootDir: path.resolve('project-2'),
    },
  ]
  const allProjects = [
    {
      buildIndex: 0,
      manifest: manifest1,
      rootDir: path.resolve('project-1'),
    },
    {
      buildIndex: 0,
      manifest: manifest2,
      rootDir: path.resolve('project-2'),
    },
  ]
  await mutateModules(importers, await testDefaults({
    allProjects,
    autoInstallPeers: false,
    dedupePeerDependents: false,
    lockfileOnly: true,
    strictPeerDependencies: false,
  }))

  const lockfile = readYamlFile<any>(path.resolve(WANTED_LOCKFILE)) // eslint-disable-line

  expect(lockfile.importers['project-1'].dependencies?.['@pnpm.e2e/has-has-y-peer-only-as-peer']['version']).not.toEqual(lockfile.importers['project-2'].dependencies?.['@pnpm.e2e/has-has-y-peer-only-as-peer']['version'])
  expect(lockfile.packages['/@pnpm.e2e/has-has-y-peer-only-as-peer@1.0.0(@pnpm.e2e/has-y-peer@1.0.0(@pnpm/y@1.0.0))'].dependencies['@pnpm.e2e/has-y-peer']).toEqual('1.0.0(@pnpm/y@1.0.0)')
})

test('peer having peer is resolved correctly. The peer is also in the dependencies of the dependent package', async () => {
  const manifest1 = {
    name: 'project-1',

    dependencies: {
      '@pnpm.e2e/has-has-y-peer-only-as-peer-and-y': '1.0.0',
      '@pnpm.e2e/has-y-peer': '1.0.0',
      '@pnpm/y': '2.0.0',
    },
  }
  const manifest2 = {
    name: 'project-2',

    dependencies: {
      '@pnpm.e2e/has-has-y-peer-only-as-peer-and-y': '1.0.0',
      '@pnpm.e2e/has-y-peer': '1.0.0',
      '@pnpm/y': '1.0.0',
    },
  }
  preparePackages([
    {
      location: 'project-1',
      package: manifest1,
    },
    {
      location: 'project-2',
      package: manifest2,
    },
  ])

  const importers: MutatedProject[] = [
    {
      mutation: 'install',
      rootDir: path.resolve('project-1'),
    },
    {
      mutation: 'install',
      rootDir: path.resolve('project-2'),
    },
  ]
  const allProjects = [
    {
      buildIndex: 0,
      manifest: manifest1,
      rootDir: path.resolve('project-1'),
    },
    {
      buildIndex: 0,
      manifest: manifest2,
      rootDir: path.resolve('project-2'),
    },
  ]
  await mutateModules(importers, await testDefaults({
    allProjects,
    autoInstallPeers: false,
    dedupePeerDependents: false,
    lockfileOnly: true,
    strictPeerDependencies: false,
  }))

  const lockfile = readYamlFile<any>(path.resolve(WANTED_LOCKFILE)) // eslint-disable-line

  expect(lockfile.importers['project-1'].dependencies?.['@pnpm.e2e/has-has-y-peer-only-as-peer-and-y']['version']).toEqual('1.0.0(@pnpm.e2e/has-y-peer@1.0.0(@pnpm/y@2.0.0))')
  expect(lockfile.importers['project-2'].dependencies?.['@pnpm.e2e/has-has-y-peer-only-as-peer-and-y']['version']).toEqual('1.0.0(@pnpm.e2e/has-y-peer@1.0.0(@pnpm/y@1.0.0))')

  expect(lockfile.packages['/@pnpm.e2e/has-has-y-peer-only-as-peer-and-y@1.0.0(@pnpm.e2e/has-y-peer@1.0.0(@pnpm/y@1.0.0))'].dependencies['@pnpm/y']).toEqual('1.0.0')
  expect(lockfile.packages['/@pnpm.e2e/has-has-y-peer-only-as-peer-and-y@1.0.0(@pnpm.e2e/has-y-peer@1.0.0(@pnpm/y@2.0.0))'].dependencies['@pnpm/y']).toEqual('1.0.0')

  expect(lockfile.packages['/@pnpm.e2e/has-has-y-peer-only-as-peer-and-y@1.0.0(@pnpm.e2e/has-y-peer@1.0.0(@pnpm/y@1.0.0))'].dependencies['@pnpm.e2e/has-y-peer']).toEqual('1.0.0(@pnpm/y@1.0.0)')
  expect(lockfile.packages['/@pnpm.e2e/has-has-y-peer-only-as-peer-and-y@1.0.0(@pnpm.e2e/has-y-peer@1.0.0(@pnpm/y@2.0.0))'].dependencies['@pnpm.e2e/has-y-peer']).toEqual('1.0.0(@pnpm/y@2.0.0)')
})

test('peer having peer is resolved correctly. The peer is also in the dependencies of the dependent package. Test #2', async () => {
  const manifest1 = {
    name: 'project-1',

    dependencies: {
      '@pnpm.e2e/has-has-y-peer-only-as-peer-and-y': '2.0.0',
      '@pnpm.e2e/has-y-peer': '1.0.0',
      '@pnpm/y': '1.0.0',
    },
  }
  const manifest2 = {
    name: 'project-2',

    dependencies: {
      '@pnpm.e2e/has-has-y-peer-only-as-peer-and-y': '1.0.0',
      '@pnpm.e2e/has-y-peer': '1.0.0',
      '@pnpm/y': '2.0.0',
    },
  }
  preparePackages([
    {
      location: 'project-1',
      package: manifest1,
    },
    {
      location: 'project-2',
      package: manifest2,
    },
  ])

  const importers: MutatedProject[] = [
    {
      mutation: 'install',
      rootDir: path.resolve('project-1'),
    },
    {
      mutation: 'install',
      rootDir: path.resolve('project-2'),
    },
  ]
  const allProjects = [
    {
      buildIndex: 0,
      manifest: manifest1,
      rootDir: path.resolve('project-1'),
    },
    {
      buildIndex: 0,
      manifest: manifest2,
      rootDir: path.resolve('project-2'),
    },
  ]
  await mutateModules(importers, await testDefaults({
    allProjects,
    autoInstallPeers: false,
    dedupePeerDependents: false,
    lockfileOnly: true,
    strictPeerDependencies: false,
  }))

  const lockfile = readYamlFile<any>(path.resolve(WANTED_LOCKFILE)) // eslint-disable-line

  expect(lockfile.packages['/@pnpm.e2e/has-has-y-peer-only-as-peer-and-y@1.0.0(@pnpm.e2e/has-y-peer@1.0.0(@pnpm/y@2.0.0))'].dependencies['@pnpm.e2e/has-y-peer']).toEqual('1.0.0(@pnpm/y@2.0.0)')
  expect(lockfile.packages['/@pnpm.e2e/has-has-y-peer-only-as-peer-and-y@2.0.0(@pnpm.e2e/has-y-peer@1.0.0(@pnpm/y@1.0.0))'].dependencies['@pnpm.e2e/has-y-peer']).toEqual('1.0.0(@pnpm/y@1.0.0)')
})

test('resolve peer of peer from the dependencies of the direct dependent package', async () => {
  const project = prepareEmpty()
  await addDependenciesToPackage({}, ['@pnpm.e2e/has-has-y-peer-only-as-peer-and-y@1.0.0', '@pnpm/y@2.0.0'], await testDefaults())

  const lockfile = project.readLockfile()

  expect(lockfile.dependencies['@pnpm.e2e/has-has-y-peer-only-as-peer-and-y'].version).toBe('1.0.0(@pnpm.e2e/has-y-peer@1.0.0(@pnpm/y@2.0.0))')
  // Even though @pnpm/y@1.0.0 is in the dependencies of the direct dependent package, we resolve y from above.
  // It might make sense to print a warning in this case and suggest to make y a peer dependency in the dependent package too.
  expect(lockfile.packages['/@pnpm.e2e/has-has-y-peer-only-as-peer-and-y@1.0.0(@pnpm.e2e/has-y-peer@1.0.0(@pnpm/y@2.0.0))'].dependencies?.['@pnpm.e2e/has-y-peer']).toBe('1.0.0(@pnpm/y@2.0.0)')
})

test('2 circular peers', async () => {
  const project = prepareEmpty()
  await addDependenciesToPackage({}, ['@pnpm.e2e/circular-peer-a@1.0.0', '@pnpm.e2e/circular-peer-b@1.0.0'], await testDefaults())

  const lockfile = project.readLockfile()

  expect(lockfile.dependencies['@pnpm.e2e/circular-peer-a'].version).toBe('1.0.0(@pnpm.e2e/circular-peer-b@1.0.0)')
  expect(lockfile.dependencies['@pnpm.e2e/circular-peer-b'].version).toBe('1.0.0(@pnpm.e2e/circular-peer-a@1.0.0)')
})

test('3 circular peers', async () => {
  const project = prepareEmpty()
  await addDependenciesToPackage({}, [
    '@pnpm.e2e/circular-peers-1-of-3@1.0.0',
    '@pnpm.e2e/circular-peers-2-of-3@1.0.0',
    '@pnpm.e2e/circular-peers-3-of-3@1.0.0',
    '@pnpm.e2e/peer-a@1.0.0',
  ], await testDefaults())

  const lockfile = project.readLockfile()

  expect(lockfile.dependencies['@pnpm.e2e/circular-peers-1-of-3'].version).toBe('1.0.0(@pnpm.e2e/circular-peers-2-of-3@1.0.0)(@pnpm.e2e/peer-a@1.0.0)')
  expect(lockfile.dependencies['@pnpm.e2e/circular-peers-2-of-3'].version).toBe('1.0.0(@pnpm.e2e/circular-peers-3-of-3@1.0.0)(@pnpm.e2e/peer-a@1.0.0)(@pnpm.e2e/peer-b@1.0.0)')
  expect(lockfile.dependencies['@pnpm.e2e/circular-peers-3-of-3'].version).toBe('1.0.0(@pnpm.e2e/circular-peers-1-of-3@1.0.0)')
})

test('3 circular peers in workspace root', async () => {
  const projects = preparePackages([
    {
      location: '.',
      package: { name: 'root' },
    },
    {
      location: 'pkg',
      package: {},
    },
  ])
  const allProjects: ProjectOptions[] = [
    {
      buildIndex: 0,
      manifest: {
        name: 'root',
        version: '1.0.0',

        dependencies: {
          '@pnpm.e2e/circular-peers-1-of-3': '1.0.0',
          '@pnpm.e2e/circular-peers-2-of-3': '1.0.0',
          '@pnpm.e2e/circular-peers-3-of-3': '1.0.0',
          '@pnpm.e2e/peer-a': '1.0.0',
        },
      },
      rootDir: process.cwd(),
    },
    {
      buildIndex: 0,
      manifest: {
        name: 'pkg',
        version: '1.0.0',

        dependencies: {
          '@pnpm.e2e/circular-peers-1-of-3': '1.0.0',
        },
      },
      rootDir: path.resolve('pkg'),
    },
  ]
  const reporter = jest.fn()
  await mutateModules([
    {
      mutation: 'install',
      rootDir: path.resolve('pkg'),
    },
    {
      mutation: 'install',
      rootDir: process.cwd(),
    },
  ], await testDefaults({ allProjects, reporter, autoInstallPeers: false, resolvePeersFromWorkspaceRoot: true, strictPeerDependencies: false }))

  const lockfile = projects.root.readLockfile()
  expect(lockfile.importers.pkg?.dependencies?.['@pnpm.e2e/circular-peers-1-of-3'].version).toBe('1.0.0(@pnpm.e2e/circular-peers-2-of-3@1.0.0(@pnpm.e2e/circular-peers-3-of-3@1.0.0)(@pnpm.e2e/peer-a@1.0.0))(@pnpm.e2e/peer-a@1.0.0)')
})

test('resolves complex circular deps', async () => {
  prepareEmpty()
  await addDependenciesToPackage({}, [
    '@pnpm.e2e/complex-circular-peers-a@1.0.0',
    '@pnpm.e2e/complex-circular-peers-b@1.0.0',
    '@pnpm.e2e/complex-circular-peers-c@1.0.0',
  ], await testDefaults({
    autoInstallPeers: false,
  }))
  // it doesn't hang
})

test('do not fail when the same package with peer dependency is installed via different aliases', async () => {
  const project = prepareEmpty()
  await addDependenciesToPackage({}, [
    '@pnpm.e2e/has-y-peer@1.0.0',
    'has-y-peer@npm:@pnpm.e2e/has-y-peer@1.0.0',
  ], await testDefaults({
    autoInstallPeers: true,
  }))
  const lockfile = project.readLockfile()
  expect(Object.keys(lockfile.packages).length).toBe(2)
})
