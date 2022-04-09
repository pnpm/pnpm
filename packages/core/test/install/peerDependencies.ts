import { promises as fs } from 'fs'
import path from 'path'
import { WANTED_LOCKFILE } from '@pnpm/constants'
import { Lockfile } from '@pnpm/lockfile-file'
import { prepareEmpty, preparePackages } from '@pnpm/prepare'
import { addDistTag, REGISTRY_MOCK_PORT } from '@pnpm/registry-mock'
import fixtures from '@pnpm/test-fixtures'
import readYamlFile from 'read-yaml-file'
import {
  addDependenciesToPackage,
  install,
  MutatedProject,
  mutateModules,
  PeerDependencyIssuesError,
} from '@pnpm/core'
import rimraf from '@zkochan/rimraf'
import exists from 'path-exists'
import sinon from 'sinon'
import deepRequireCwd from 'deep-require-cwd'
import { testDefaults } from '../utils'

const f = fixtures(__dirname)

test("don't fail when peer dependency is fetched from GitHub", async () => {
  prepareEmpty()
  await addDependenciesToPackage({}, ['test-pnpm-peer-deps'], await testDefaults())
})

test('peer dependency is grouped with dependency when peer is resolved not from a top dependency', async () => {
  const project = prepareEmpty()
  const opts = await testDefaults()
  let manifest = await addDependenciesToPackage({}, ['using-ajv'], opts)

  expect(await exists(path.resolve('node_modules/.pnpm/ajv-keywords@1.5.0_ajv@4.10.4/node_modules/ajv'))).toBeTruthy()
  expect(deepRequireCwd(['using-ajv', 'ajv-keywords', 'ajv', './package.json']).version).toBe('4.10.4')

  // testing that peers are reinstalled correctly using info from the lockfile
  await rimraf('node_modules')
  await rimraf(path.resolve('..', '.store'))
  manifest = await install(manifest, await testDefaults())

  expect(await exists(path.resolve('node_modules/.pnpm/ajv-keywords@1.5.0_ajv@4.10.4/node_modules/ajv'))).toBeTruthy()
  expect(deepRequireCwd(['using-ajv', 'ajv-keywords', 'ajv', './package.json']).version).toBe('4.10.4')

  await addDependenciesToPackage(manifest, ['using-ajv'], await testDefaults({ update: true }))

  const lockfile = await project.readLockfile()

  expect(lockfile.packages['/using-ajv/1.0.0'].dependencies!['ajv-keywords']).toBe('1.5.0_ajv@4.10.4')
  // covers https://github.com/pnpm/pnpm/issues/1150
  expect(lockfile.packages).toHaveProperty(['/ajv-keywords/1.5.0_ajv@4.10.4'])
})

// Covers https://github.com/pnpm/pnpm/issues/1133
test('nothing is needlessly removed from node_modules', async () => {
  prepareEmpty()
  const opts = await testDefaults({
    modulesCacheMaxAge: 0,
    strictPeerDependencies: false,
  })
  const manifest = await addDependenciesToPackage({}, ['using-ajv', 'ajv-keywords@1.5.0'], opts)

  expect(await exists(path.resolve('node_modules/.pnpm/ajv-keywords@1.5.0_ajv@4.10.4/node_modules/ajv'))).toBeTruthy()
  expect(await exists(path.resolve('node_modules/.pnpm/ajv-keywords@1.5.0/node_modules/ajv-keywords'))).toBeTruthy()
  expect(deepRequireCwd(['using-ajv', 'ajv-keywords', 'ajv', './package.json']).version).toBe('4.10.4')

  await mutateModules([
    {
      dependencyNames: ['ajv-keywords'],
      manifest,
      mutation: 'uninstallSome',
      rootDir: process.cwd(),
    },
  ], opts)

  expect(await exists(path.resolve('node_modules/.pnpm/ajv-keywords@1.5.0_ajv@4.10.4/node_modules/ajv'))).toBeTruthy()
  expect(await exists(path.resolve('node_modules/.pnpm/ajv-keywords@1.5.0/node_modules/ajv-keywords'))).toBeFalsy()
})

test('peer dependency is grouped with dependent when the peer is a top dependency', async () => {
  const project = prepareEmpty()

  const reporter = sinon.spy()

  const manifest = await addDependenciesToPackage({}, ['ajv@4.10.4', 'ajv-keywords@1.5.0'], await testDefaults({ reporter }))

  expect(reporter.calledWithMatch({
    message: `localhost+${REGISTRY_MOCK_PORT}/ajv-keywords/1.5.0 requires a peer of ajv@>=4.10.0 but none was installed.`,
  })).toBeFalsy()

  expect(await exists(path.resolve('node_modules/.pnpm/ajv-keywords@1.5.0_ajv@4.10.4/node_modules/ajv-keywords'))).toBeTruthy()

  await mutateModules([
    {
      buildIndex: 0,
      manifest,
      mutation: 'install',
      rootDir: process.cwd(),
    },
  ], await testDefaults({ preferFrozenLockfile: false }))

  const lockfile = await project.readLockfile()
  expect(lockfile.packages['/ajv-keywords/1.5.0_ajv@4.10.4'].dependencies).toHaveProperty(['ajv'])
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
      buildIndex: 0,
      manifest: manifest1,
      mutation: 'install',
      rootDir: path.resolve('project-1'),
    },
    {
      buildIndex: 0,
      manifest: manifest2,
      mutation: 'install',
      rootDir: path.resolve('project-2'),
    },
  ]
  await mutateModules(importers, await testDefaults({ lockfileOnly: true, strictPeerDependencies: false }))

  const lockfile = await readYamlFile<Lockfile>(path.resolve(WANTED_LOCKFILE))

  expect(lockfile.importers['project-1'].dependencies).toStrictEqual({
    'ajv-keywords': '1.5.0',
  })
  expect(lockfile.importers['project-2'].dependencies).toStrictEqual({
    ajv: '4.10.4',
    'ajv-keywords': '1.5.0_ajv@4.10.4',
  })
})

test('warning is reported when cannot resolve peer dependency for top-level dependency', async () => {
  prepareEmpty()

  const reporter = jest.fn()

  await addDependenciesToPackage({}, ['ajv-keywords@1.5.0'], await testDefaults({ reporter, strictPeerDependencies: false }))

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
    await addDependenciesToPackage({}, ['ajv-keywords@1.5.0'], await testDefaults({ strictPeerDependencies: true }))
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

test('warning is reported when cannot resolve peer dependency for non-top-level dependency', async () => {
  prepareEmpty()
  await addDistTag({ package: 'abc-parent-with-ab', version: '1.0.0', distTag: 'latest' })

  const reporter = jest.fn()

  await addDependenciesToPackage({}, ['abc-grand-parent-without-c'], await testDefaults({ reporter, strictPeerDependencies: false }))

  expect(reporter).toHaveBeenCalledWith(
    expect.objectContaining({
      level: 'debug',
      name: 'pnpm:peer-dependency-issues',
      issuesByProjects: {
        '.': {
          bad: {},
          missing: {
            'peer-c': [
              {
                parents: [
                  {
                    name: 'abc-grand-parent-without-c',
                    version: '1.0.0',
                  },
                  {
                    name: 'abc-parent-with-ab',
                    version: '1.0.0',
                  },
                  {
                    name: 'abc',
                    version: '1.0.0',
                  },
                ],
                optional: false,
                wantedRange: '^1.0.0',
              },
            ],
          },
          conflicts: [],
          intersections: { 'peer-c': '^1.0.0' },
        },
      },
    })
  )
})

test('warning is reported when bad version of resolved peer dependency for non-top-level dependency', async () => {
  await addDistTag({ package: 'abc-parent-with-ab', version: '1.0.0', distTag: 'latest' })
  prepareEmpty()

  const reporter = jest.fn()

  await addDependenciesToPackage({}, ['abc-grand-parent-without-c', 'peer-c@2'], await testDefaults({ reporter, strictPeerDependencies: false }))

  expect(reporter).toHaveBeenCalledWith(
    expect.objectContaining({
      level: 'debug',
      name: 'pnpm:peer-dependency-issues',
      issuesByProjects: {
        '.': {
          bad: {
            'peer-c': [
              {
                parents: [
                  {
                    name: 'abc-grand-parent-without-c',
                    version: '1.0.0',
                  },
                  {
                    name: 'abc-parent-with-ab',
                    version: '1.0.0',
                  },
                  {
                    name: 'abc',
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
  await addDistTag({ package: 'abc-parent-with-ab', version: '1.0.0', distTag: 'latest' })
  prepareEmpty()

  let err!: PeerDependencyIssuesError
  try {
    await addDependenciesToPackage({}, ['abc-grand-parent-without-c', 'peer-c@2'], await testDefaults({ strictPeerDependencies: true }))
  } catch (_err: any) { // eslint-disable-line
    err = _err
  }

  expect(err?.issuesByProjects['.']).toStrictEqual({
    bad: {
      'peer-c': [
        {
          parents: [
            {
              name: 'abc-grand-parent-without-c',
              version: '1.0.0',
            },
            {
              name: 'abc-parent-with-ab',
              version: '1.0.0',
            },
            {
              name: 'abc',
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

  const manifest = await addDependenciesToPackage({}, ['peer-c@1.0.0'], await testDefaults())

  await addDependenciesToPackage(manifest, ['abc-parent-with-ab@1.0.0'], await testDefaults())

  expect(await exists(path.resolve('node_modules/.pnpm/abc-parent-with-ab@1.0.0/node_modules/abc-parent-with-ab'))).toBeFalsy()
  expect(await exists(path.resolve('node_modules/.pnpm/abc-parent-with-ab@1.0.0_peer-c@1.0.0/node_modules/abc-parent-with-ab'))).toBeTruthy()
})

test('top peer dependency is linked on subsequent install, through transitive peer', async () => {
  prepareEmpty()

  const manifest = await addDependenciesToPackage({}, ['abc-grand-parent@1.0.0'], await testDefaults({ strictPeerDependencies: false }))

  await addDependenciesToPackage(manifest, ['peer-c@1.0.0'], await testDefaults({ strictPeerDependencies: false }))

  expect(await exists(path.resolve('node_modules/.pnpm/abc-grand-parent@1.0.0_peer-c@1.0.0/node_modules/abc-grand-parent'))).toBeTruthy()
})

test('the list of transitive peer dependencies is kept up to date', async () => {
  const project = prepareEmpty()
  await addDistTag({ package: 'abc-parent-with-ab', version: '1.0.0', distTag: 'latest' })

  const manifest = await addDependenciesToPackage({}, ['abc-grand-parent@1.0.0', 'peer-c@1.0.0'], await testDefaults())

  await addDistTag({ package: 'abc-parent-with-ab', version: '1.1.0', distTag: 'latest' })

  expect(await exists(path.resolve('node_modules/.pnpm/abc-grand-parent@1.0.0_peer-c@1.0.0/node_modules/abc-grand-parent'))).toBeTruthy()
  {
    const lockfile = await project.readLockfile()
    expect(lockfile.packages['/abc-grand-parent/1.0.0_peer-c@1.0.0'].transitivePeerDependencies).toStrictEqual(['peer-c'])
  }

  await mutateModules([
    {
      buildIndex: 0,
      manifest,
      mutation: 'install',
      rootDir: process.cwd(),
    },
  ], await testDefaults({ update: true, depth: Infinity }))

  expect(await exists(path.resolve('node_modules/.pnpm/abc-grand-parent@1.0.0/node_modules/abc-grand-parent'))).toBeTruthy()

  {
    const lockfile = await project.readLockfile()
    expect(lockfile.packages['/abc-grand-parent/1.0.0'].transitivePeerDependencies).toBeFalsy()
  }
})

test('top peer dependency is linked on subsequent install. Reverse order', async () => {
  prepareEmpty()

  const manifest = await addDependenciesToPackage({}, ['abc-parent-with-ab@1.0.0'], await testDefaults({ strictPeerDependencies: false }))

  await addDependenciesToPackage(manifest, ['peer-c@1.0.0'], await testDefaults({ modulesCacheMaxAge: 0, strictPeerDependencies: false }))

  expect(await exists(path.resolve('node_modules/.pnpm/abc-parent-with-ab@1.0.0/node_modules/abc-parent-with-ab'))).toBeFalsy()
  expect(await exists(path.resolve('node_modules/.pnpm/abc-parent-with-ab@1.0.0_peer-c@1.0.0/node_modules/abc-parent-with-ab'))).toBeTruthy()
  expect(await exists(path.resolve('node_modules/.pnpm/abc-parent-with-ab@1.0.0_peer-c@1.0.0/node_modules/is-positive'))).toBeTruthy()
})

async function okFile (filename: string) {
  expect(await exists(filename)).toBeTruthy()
}

// This usecase was failing. See https://github.com/pnpm/supi/issues/15
test('peer dependencies are linked when running one named installation', async () => {
  await addDistTag({ package: 'abc-parent-with-ab', version: '1.0.1', distTag: 'latest' })
  await addDistTag({ package: 'peer-a', version: '1.0.0', distTag: 'latest' })
  await addDistTag({ package: 'peer-c', version: '1.0.0', distTag: 'latest' })

  prepareEmpty()

  const manifest = await addDependenciesToPackage({}, ['abc-grand-parent-with-c', 'abc-parent-with-ab', 'peer-c@2.0.0'], await testDefaults({ strictPeerDependencies: false }))

  const pkgVariationsDir = path.resolve('node_modules/.pnpm/abc@1.0.0')

  const pkgVariation1 = path.join(pkgVariationsDir + '_165e1e08a3f7e7f77ddb572ad0e55660/node_modules')
  await okFile(path.join(pkgVariation1, 'abc'))
  await okFile(path.join(pkgVariation1, 'peer-a'))
  await okFile(path.join(pkgVariation1, 'peer-b'))
  await okFile(path.join(pkgVariation1, 'peer-c'))
  await okFile(path.join(pkgVariation1, 'dep-of-pkg-with-1-dep'))

  const pkgVariation2 = path.join(pkgVariationsDir + '_f101cfec1621b915239e5c82246da43c/node_modules')
  await okFile(path.join(pkgVariation2, 'abc'))
  await okFile(path.join(pkgVariation2, 'peer-a'))
  await okFile(path.join(pkgVariation2, 'peer-b'))
  await okFile(path.join(pkgVariation2, 'peer-c'))
  await okFile(path.join(pkgVariation2, 'dep-of-pkg-with-1-dep'))

  expect(deepRequireCwd(['abc-parent-with-ab', 'abc', 'peer-c', './package.json']).version).toBe('2.0.0')
  expect(deepRequireCwd(['abc-grand-parent-with-c', 'abc-parent-with-ab', 'abc', 'peer-c', './package.json']).version).toBe('1.0.0')

  // this part was failing. See issue: https://github.com/pnpm/pnpm/issues/1201
  await addDistTag({ package: 'peer-a', version: '1.0.1', distTag: 'latest' })
  await install(manifest, await testDefaults({ update: true, depth: 100, strictPeerDependencies: false }))
})

test('peer dependencies are linked when running two separate named installations', async () => {
  await addDistTag({ package: 'peer-a', version: '1.0.0', distTag: 'latest' })
  await addDistTag({ package: 'peer-c', version: '1.0.0', distTag: 'latest' })
  prepareEmpty()

  const manifest = await addDependenciesToPackage({}, ['abc-grand-parent-with-c', 'peer-c@2.0.0'], await testDefaults({ strictPeerDependencies: false }))
  await addDependenciesToPackage(manifest, ['abc-parent-with-ab'], await testDefaults({ strictPeerDependencies: false }))

  const pkgVariationsDir = path.resolve('node_modules/.pnpm/abc@1.0.0')

  const pkgVariation1 = path.join(pkgVariationsDir + '_165e1e08a3f7e7f77ddb572ad0e55660/node_modules')
  await okFile(path.join(pkgVariation1, 'abc'))
  await okFile(path.join(pkgVariation1, 'peer-a'))
  await okFile(path.join(pkgVariation1, 'peer-b'))
  await okFile(path.join(pkgVariation1, 'peer-c'))
  await okFile(path.join(pkgVariation1, 'dep-of-pkg-with-1-dep'))

  const pkgVariation2 = path.join(pkgVariationsDir + '_165e1e08a3f7e7f77ddb572ad0e55660/node_modules')
  await okFile(path.join(pkgVariation2, 'abc'))
  await okFile(path.join(pkgVariation2, 'peer-a'))
  await okFile(path.join(pkgVariation2, 'peer-b'))
  await okFile(path.join(pkgVariation2, 'dep-of-pkg-with-1-dep'))

  expect(deepRequireCwd(['abc-parent-with-ab', 'abc', 'peer-c', './package.json']).version).toBe('2.0.0')
  expect(deepRequireCwd(['abc-grand-parent-with-c', 'abc-parent-with-ab', 'abc', 'peer-c', './package.json']).version).toBe('1.0.0')
})

// eslint-disable-next-line @typescript-eslint/dot-notation
test.skip('peer dependencies are linked', async () => {
  const project = prepareEmpty()
  await install({
    dependencies: {
      'abc-grand-parent-with-c': '*',
      'peer-c': '2.0.0',
    },
    devDependencies: {
      'abc-parent-with-ab': '*',
    },
  }, await testDefaults())

  const pkgVariationsDir = path.resolve('node_modules/.pnpm/abc@1.0.0')

  const pkgVariation1 = path.join(pkgVariationsDir, '165e1e08a3f7e7f77ddb572ad0e55660/node_modules')
  await okFile(path.join(pkgVariation1, 'abc'))
  await okFile(path.join(pkgVariation1, 'peer-a'))
  await okFile(path.join(pkgVariation1, 'peer-b'))
  await okFile(path.join(pkgVariation1, 'peer-c'))
  await okFile(path.join(pkgVariation1, 'dep-of-pkg-with-1-dep'))

  const pkgVariation2 = path.join(pkgVariationsDir, 'peer-a@1.0.0+peer-b@1.0.0/node_modules')
  await okFile(path.join(pkgVariation2, 'abc'))
  await okFile(path.join(pkgVariation2, 'peer-a'))
  await okFile(path.join(pkgVariation2, 'peer-b'))
  await okFile(path.join(pkgVariation2, 'dep-of-pkg-with-1-dep'))

  expect(deepRequireCwd(['abc-parent-with-ab', 'abc', 'peer-c', './package.json']).version).toBe('2.0.0')
  expect(deepRequireCwd(['abc-grand-parent-with-c', 'abc-parent-with-ab', 'abc', 'peer-c', './package.json']).version).toBe('1.0.0')

  const lockfile = await project.readLockfile()
  expect(lockfile.packages['/abc-parent-with-ab/1.0.0/peer-a@1.0.0+peer-b@1.0.0'].dev).toBeTruthy()
})

test('scoped peer dependency is linked', async () => {
  prepareEmpty()
  await addDependenciesToPackage({}, ['for-testing-scoped-peers'], await testDefaults())

  const pkgVariation = path.resolve('node_modules/.pnpm/@having+scoped-peer@1.0.0_@scoped+peer@1.0.0/node_modules')
  await okFile(path.join(pkgVariation, '@having', 'scoped-peer'))
  await okFile(path.join(pkgVariation, '@scoped', 'peer'))
})

test('peer bins are linked', async () => {
  const project = prepareEmpty()

  await addDependenciesToPackage({}, ['for-testing-peers-having-bins'], await testDefaults({ fastUnpack: false }))

  const pkgVariation = path.join('.pnpm/pkg-with-peer-having-bin@1.0.0_peer-with-bin@1.0.0/node_modules')

  await project.isExecutable(path.join(pkgVariation, 'pkg-with-peer-having-bin/node_modules/.bin', 'peer-with-bin'))

  await project.isExecutable(path.join(pkgVariation, 'pkg-with-peer-having-bin/node_modules/.bin', 'hello-world-js-bin'))
})

test('run pre/postinstall scripts of each variations of packages with peer dependencies', async () => {
  await addDistTag({ package: 'peer-c', version: '1.0.0', distTag: 'latest' })
  prepareEmpty()
  await addDependenciesToPackage({}, ['parent-of-pkg-with-events-and-peers', 'pkg-with-events-and-peers', 'peer-c@2.0.0'], await testDefaults({ fastUnpack: false }))

  const pkgVariation1 = path.resolve('node_modules/.pnpm/pkg-with-events-and-peers@1.0.0_peer-c@1.0.0/node_modules')
  await okFile(path.join(pkgVariation1, 'pkg-with-events-and-peers', 'generated-by-preinstall.js'))
  await okFile(path.join(pkgVariation1, 'pkg-with-events-and-peers', 'generated-by-postinstall.js'))

  const pkgVariation2 = path.resolve('node_modules/.pnpm/pkg-with-events-and-peers@1.0.0_peer-c@2.0.0/node_modules')
  await okFile(path.join(pkgVariation2, 'pkg-with-events-and-peers', 'generated-by-preinstall.js'))
  await okFile(path.join(pkgVariation2, 'pkg-with-events-and-peers', 'generated-by-postinstall.js'))
})

test('package that resolves its own peer dependency', async () => {
  // TODO: investigate how npm behaves in such situations
  // should there be a warning printed?
  // does it currently print a warning that peer dependency is not resolved?

  await addDistTag({ package: 'peer-c', version: '1.0.0', distTag: 'latest' })
  const project = prepareEmpty()
  await addDependenciesToPackage({}, ['pkg-with-resolved-peer', 'peer-c@2.0.0'], await testDefaults())

  expect(deepRequireCwd(['pkg-with-resolved-peer', 'peer-c', './package.json']).version).toBe('1.0.0')

  expect(await exists(path.resolve('node_modules/.pnpm/pkg-with-resolved-peer@1.0.0/node_modules/pkg-with-resolved-peer'))).toBeTruthy()

  const lockfile = await project.readLockfile()

  expect(lockfile.packages['/pkg-with-resolved-peer/1.0.0']).not.toHaveProperty(['peerDependencies'])
  expect(lockfile.packages['/pkg-with-resolved-peer/1.0.0'].dependencies).toHaveProperty(['peer-c'])
  expect(lockfile.packages['/pkg-with-resolved-peer/1.0.0'].optionalDependencies).toHaveProperty(['peer-b'])
})

test('package that has parent as peer dependency', async () => {
  const project = prepareEmpty()
  await addDependenciesToPackage({}, ['has-alpha', 'alpha'], await testDefaults())

  const lockfile = await project.readLockfile()

  expect(lockfile.packages).toHaveProperty(['/has-alpha-as-peer/1.0.0_alpha@1.0.0'])
  expect(lockfile.packages).not.toHaveProperty(['/has-alpha-as-peer/1.0.0'])
})

test('own peer installed in root as well is linked to root', async () => {
  prepareEmpty()

  await addDependenciesToPackage({}, ['is-negative@kevva/is-negative#2.1.0', 'peer-deps-in-child-pkg'], await testDefaults())

  expect(deepRequireCwd.silent(['is-negative', './package.json'])).toBeTruthy()
})

test('peer dependency is grouped with dependent when the peer is a top dependency but an external lockfile is used', async () => {
  prepareEmpty()

  const reporter = sinon.spy()

  await addDependenciesToPackage({}, ['ajv@4.10.4', 'ajv-keywords@1.5.0'], await testDefaults({ reporter, lockfileDir: path.resolve('..'), strictPeerDependencies: false }))

  expect(reporter.calledWithMatch({
    message: `localhost+${REGISTRY_MOCK_PORT}/ajv-keywords@1.5.0 requires a peer of ajv@>=4.10.0 but none was installed.`,
  })).toBeFalsy()

  expect(await exists(path.join('../node_modules/.pnpm/ajv-keywords@1.5.0_ajv@4.10.4/node_modules/ajv-keywords'))).toBeTruthy()

  const lockfile = await readYamlFile<Lockfile>(path.join('..', WANTED_LOCKFILE))

  expect(lockfile.importers.project).toStrictEqual({ // eslint-disable-line
    dependencies: {
      ajv: '4.10.4',
      'ajv-keywords': '1.5.0_ajv@4.10.4',
    },
    specifiers: {
      ajv: '4.10.4',
      'ajv-keywords': '1.5.0',
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
      abc: '1.0.0',
    },
  }, await testDefaults({ reporter, lockfileDir, strictPeerDependencies: false }))
  await addDependenciesToPackage(manifest, ['peer-c@2.0.0'], await testDefaults({ reporter, lockfileDir, strictPeerDependencies: false }))

  expect(await exists(path.join('../node_modules/.pnpm/abc@1.0.0_peer-c@2.0.0/node_modules/dep-of-pkg-with-1-dep'))).toBeTruthy()
})

test('peer dependency is grouped with dependent when the peer is a top dependency and external node_modules is used', async () => {
  prepareEmpty()
  await fs.mkdir('_')
  process.chdir('_')
  const lockfileDir = path.resolve('..')

  let manifest = await addDependenciesToPackage({}, ['ajv@4.10.4', 'ajv-keywords@1.5.0'], await testDefaults({ lockfileDir, strictPeerDependencies: false }))

  {
    const lockfile = await readYamlFile<Lockfile>(path.resolve('..', WANTED_LOCKFILE))
    expect(lockfile.importers._).toStrictEqual({
      dependencies: {
        ajv: '4.10.4',
        'ajv-keywords': '1.5.0_ajv@4.10.4',
      },
      specifiers: {
        ajv: '4.10.4',
        'ajv-keywords': '1.5.0',
      },
    })
  }

  manifest = await install(manifest, await testDefaults({ lockfileDir, strictPeerDependencies: false }))

  {
    const lockfile = await readYamlFile<Lockfile>(path.resolve('..', WANTED_LOCKFILE))
    expect(lockfile.importers._).toStrictEqual({
      dependencies: {
        ajv: '4.10.4',
        'ajv-keywords': '1.5.0_ajv@4.10.4',
      },
      specifiers: {
        ajv: '4.10.4',
        'ajv-keywords': '1.5.0',
      },
    })
  }

  // Covers https://github.com/pnpm/pnpm/issues/1506
  await mutateModules(
    [
      {
        dependencyNames: ['ajv'],
        manifest,
        mutation: 'uninstallSome',
        rootDir: process.cwd(),
      },
    ],
    await testDefaults({
      lockfileDir,
      strictPeerDependencies: false,
    })
  )

  {
    const lockfile = await readYamlFile<Lockfile>(path.resolve('..', WANTED_LOCKFILE))
    expect(lockfile.importers._).toStrictEqual({
      dependencies: {
        'ajv-keywords': '1.5.0',
      },
      specifiers: {
        'ajv-keywords': '1.5.0',
      },
    })
  }
})

test('external lockfile: peer dependency is grouped with dependent even after a named update', async () => {
  prepareEmpty()
  await fs.mkdir('_')
  process.chdir('_')
  const lockfileDir = path.resolve('..')

  const manifest = await addDependenciesToPackage({}, ['ajv@4.10.4', 'ajv-keywords@1.4.0'], await testDefaults({ lockfileDir }))

  {
    const lockfile = await readYamlFile<Lockfile>(path.resolve('..', WANTED_LOCKFILE))
    expect(lockfile.importers._).toStrictEqual({
      dependencies: {
        ajv: '4.10.4',
        'ajv-keywords': '1.4.0_ajv@4.10.4',
      },
      specifiers: {
        ajv: '4.10.4',
        'ajv-keywords': '1.4.0',
      },
    })
  }

  await addDependenciesToPackage(manifest, ['ajv-keywords@1.5.0'], await testDefaults({ lockfileDir }))

  {
    const lockfile = await readYamlFile<Lockfile>(path.resolve('..', WANTED_LOCKFILE))
    expect(lockfile.importers._).toStrictEqual({
      dependencies: {
        ajv: '4.10.4',
        'ajv-keywords': '1.5.0_ajv@4.10.4',
      },
      specifiers: {
        ajv: '4.10.4',
        'ajv-keywords': '1.5.0',
      },
    })
  }
})

test('external lockfile: peer dependency is grouped with dependent even after a named update of the resolved package', async () => {
  prepareEmpty()
  await fs.mkdir('_')
  process.chdir('_')
  const lockfileDir = path.resolve('..')

  const manifest = await addDependenciesToPackage({}, ['peer-c@1.0.0', 'abc-parent-with-ab@1.0.0'], await testDefaults({ lockfileDir, strictPeerDependencies: false }))

  {
    const lockfile = await readYamlFile<Lockfile>(path.resolve('..', WANTED_LOCKFILE))
    expect(lockfile.importers._).toStrictEqual({
      dependencies: {
        'abc-parent-with-ab': '1.0.0_peer-c@1.0.0',
        'peer-c': '1.0.0',
      },
      specifiers: {
        'abc-parent-with-ab': '1.0.0',
        'peer-c': '1.0.0',
      },
    })
  }

  await addDependenciesToPackage(manifest, ['peer-c@2.0.0'], await testDefaults({ lockfileDir, strictPeerDependencies: false }))

  {
    const lockfile = await readYamlFile<Lockfile>(path.resolve('..', WANTED_LOCKFILE))
    expect(lockfile.importers._).toStrictEqual({
      dependencies: {
        'abc-parent-with-ab': '1.0.0_peer-c@2.0.0',
        'peer-c': '2.0.0',
      },
      specifiers: {
        'abc-parent-with-ab': '1.0.0',
        'peer-c': '2.0.0',
      },
    })
  }

  expect(await exists(path.join('../node_modules/.pnpm/abc-parent-with-ab@1.0.0_peer-c@2.0.0/node_modules/is-positive'))).toBeTruthy()
})

test('regular dependencies are not removed on update from transitive packages that have children with peers resolved from above', async () => {
  prepareEmpty()
  await fs.mkdir('_')
  process.chdir('_')
  const lockfileDir = path.resolve('..')
  await addDistTag({ package: 'abc-parent-with-ab', version: '1.0.1', distTag: 'latest' })
  await addDistTag({ package: 'peer-c', version: '1.0.0', distTag: 'latest' })

  const manifest = await addDependenciesToPackage({}, ['abc-grand-parent-with-c@1.0.0'], await testDefaults({ lockfileDir }))

  await addDistTag({ package: 'peer-c', version: '1.0.1', distTag: 'latest' })
  await install(manifest, await testDefaults({ lockfileDir, update: true, depth: 2 }))

  expect(await exists(path.join('../node_modules/.pnpm/abc-parent-with-ab@1.0.1_peer-c@1.0.1/node_modules/is-positive'))).toBeTruthy()
})

test('peer dependency is resolved from parent package', async () => {
  preparePackages([
    {
      name: 'pkg',
    },
  ])
  await mutateModules([
    {
      dependencySelectors: ['tango@1.0.0'],
      manifest: {},
      mutation: 'installSome',
      rootDir: path.resolve('pkg'),
    },
  ], await testDefaults())

  const lockfile = await readYamlFile<Lockfile>(WANTED_LOCKFILE)
  expect(Object.keys(lockfile.packages ?? {})).toStrictEqual([
    '/has-tango-as-peer-dep/1.0.0_tango@1.0.0',
    '/tango/1.0.0',
  ])
})

test('transitive peerDependencies field does not break the lockfile on subsequent named install', async () => {
  preparePackages([
    {
      name: 'pkg',
    },
  ])
  const [{ manifest }] = await mutateModules([
    {
      dependencySelectors: ['most@1.7.3'],
      manifest: {},
      mutation: 'installSome',
      rootDir: path.resolve('pkg'),
    },
  ], await testDefaults())

  await mutateModules([
    {
      dependencySelectors: ['is-positive'],
      manifest,
      mutation: 'installSome',
      rootDir: path.resolve('pkg'),
    },
  ], await testDefaults())

  const lockfile = await readYamlFile<Lockfile>(WANTED_LOCKFILE)

  expect(Object.keys(lockfile.packages!['/most/1.7.3'].dependencies!)).toStrictEqual([
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
  await mutateModules([
    {
      dependencySelectors: ['tango@npm:tango-tango@1.0.0'],
      manifest: {},
      mutation: 'installSome',
      rootDir: path.resolve('pkg'),
    },
  ], await testDefaults())

  const lockfile = await readYamlFile<Lockfile>(WANTED_LOCKFILE)
  expect(Object.keys(lockfile.packages ?? {})).toStrictEqual([
    '/has-tango-as-peer-dep/1.0.0_tango-tango@1.0.0',
    '/tango-tango/1.0.0_tango-tango@1.0.0',
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

  const [mutatedImporter] = await mutateModules([
    {
      dependencyNames: ['is-positive'],
      manifest,
      mutation: 'uninstallSome',
      rootDir: process.cwd(),
    },
  ], await testDefaults())

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

  await addDependenciesToPackage({}, ['abc-optional-peers@1.0.0', 'peer-c@2.0.0'], await testDefaults({ reporter, strictPeerDependencies: false }))

  expect(reporter).toHaveBeenCalledWith(
    expect.objectContaining({
      level: 'debug',
      name: 'pnpm:peer-dependency-issues',
      issuesByProjects: {
        '.': {
          bad: {
            'peer-c': [{
              parents: [
                {
                  name: 'abc-optional-peers',
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
            'peer-a': [
              {
                parents: [
                  {
                    name: 'abc-optional-peers',
                    version: '1.0.0',
                  },
                ],
                optional: false,
                wantedRange: '^1.0.0',
              },
            ],
            'peer-b': [
              {
                parents: [
                  {
                    name: 'abc-optional-peers',
                    version: '1.0.0',
                  },
                ],
                optional: true,
                wantedRange: '^1.0.0',
              },
            ],
          },
          conflicts: [],
          intersections: { 'peer-a': '^1.0.0' },
        },
      },
    })
  )

  const lockfile = await project.readLockfile()

  expect(lockfile.packages['/abc-optional-peers/1.0.0_peer-c@2.0.0'].peerDependenciesMeta).toStrictEqual({
    'peer-b': {
      optional: true,
    },
    'peer-c': {
      optional: true,
    },
  })
})

test('warning is not reported when cannot resolve optional peer dependency (specified by meta field only)', async () => {
  const project = prepareEmpty()

  const reporter = jest.fn()

  await addDependenciesToPackage({}, ['abc-optional-peers-meta-only@1.0.0', 'peer-c@2.0.0'], await testDefaults({ reporter, strictPeerDependencies: false }))

  expect(reporter).toHaveBeenCalledWith(
    expect.objectContaining({
      level: 'debug',
      name: 'pnpm:peer-dependency-issues',
      issuesByProjects: {
        '.': {
          bad: {},
          missing: {
            'peer-a': [
              {
                parents: [
                  {
                    name: 'abc-optional-peers-meta-only',
                    version: '1.0.0',
                  },
                ],
                optional: false,
                wantedRange: '^1.0.0',
              },
            ],
            'peer-b': [
              {
                parents: [
                  {
                    name: 'abc-optional-peers-meta-only',
                    version: '1.0.0',
                  },
                ],
                optional: true,
                wantedRange: '*',
              },
            ],
          },
          conflicts: [],
          intersections: { 'peer-a': '^1.0.0' },
        },
      },
    })
  )

  const lockfile = await project.readLockfile()

  expect(lockfile.packages['/abc-optional-peers-meta-only/1.0.0_peer-c@2.0.0'].peerDependencies).toStrictEqual({
    'peer-a': '^1.0.0',
    'peer-b': '*',
    'peer-c': '*',
  })
  expect(lockfile.packages['/abc-optional-peers-meta-only/1.0.0_peer-c@2.0.0'].peerDependenciesMeta).toStrictEqual({
    'peer-b': {
      optional: true,
    },
    'peer-c': {
      optional: true,
    },
  })
})

test('local tarball dependency with peer dependency', async () => {
  prepareEmpty()

  const reporter = sinon.spy()

  const manifest = await addDependenciesToPackage({}, [
    `file:${f.find('tar-pkg-with-peers/tar-pkg-with-peers-1.0.0.tgz')}`,
    'bar@100.0.0',
    'foo@100.0.0',
  ], await testDefaults({ reporter }))

  const integrityLocalPkgDirs = (await fs.readdir('node_modules/.pnpm'))
    .filter((dir) => dir.startsWith('file+'))

  expect(integrityLocalPkgDirs.length).toBe(1)

  await rimraf('node_modules')

  await mutateModules([
    {
      buildIndex: 0,
      manifest,
      mutation: 'install',
      rootDir: process.cwd(),
    },
  ], await testDefaults())

  {
    const updatedLocalPkgDirs = (await fs.readdir('node_modules/.pnpm'))
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

  await mutateModules([
    {
      buildIndex: 0,
      manifest,
      mutation: 'install',
      rootDir: process.cwd(),
    },
  ], await testDefaults({ fastUnpack: false, lockfileOnly: true, strictPeerDependencies: false }))

  const lockfile = await project.readLockfile()
  expect(lockfile.packages['/@types/mongoose/5.7.32'].dev).toBeTruthy()

  await mutateModules([
    {
      buildIndex: 0,
      manifest,
      mutation: 'install',
      rootDir: process.cwd(),
    },
  ], await testDefaults({
    frozenLockfile: true,
    include: {
      dependencies: true,
      devDependencies: false,
      optionalDependencies: false,
    },
  }))

  await project.has('@typegoose/typegoose')
  await project.hasNot('@types/mongoose')
})

test('peer dependency is grouped with dependency when peer is resolved not from a top dependency', async () => {
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
      buildIndex: 0,
      manifest: project1Manifest,
      mutation: 'install',
      rootDir: path.resolve('project-1'),
    },
    {
      buildIndex: 0,
      manifest: project2Manifest,
      mutation: 'install',
      rootDir: path.resolve('ajv'),
    },
  ]
  await mutateModules(importers, await testDefaults({}))

  const lockfile = await readYamlFile<Lockfile>(path.resolve(WANTED_LOCKFILE))
  expect(lockfile.packages?.['/ajv-keywords/1.5.0_ajv@ajv'].dependencies?.['ajv']).toBe('link:ajv')
})
