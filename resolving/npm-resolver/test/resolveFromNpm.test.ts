/// <reference path="../../../__typings__/index.d.ts"/>
import { createFetchFromRegistry } from '@pnpm/fetch'
import { createNpmResolver } from '@pnpm/npm-resolver'
import { type Registries } from '@pnpm/types'
import { type WantedDependency } from '@pnpm/resolver-base'
import nock from 'nock'
import { temporaryDirectory } from 'tempy'

const registries = {
  default: 'https://registry.npmjs.org/',
} satisfies Registries

const fetch = createFetchFromRegistry({})
const getAuthHeader = () => undefined
const createResolveFromNpm = createNpmResolver.bind(null, fetch, getAuthHeader)

afterEach(() => {
  nock.cleanAll()
  nock.disableNetConnect()
})

beforeEach(() => {
  nock.enableNetConnect()
})

describe('resolveFromNpm', () => {
  test('should use full metadata for optional dependencies', async () => {
    const biomejsBiomeMetaFull = {
      name: '@biomejs/biome',
      'dist-tags': { latest: '2.3.11' },
      versions: {
        '2.3.11': {
          name: '@biomejs/biome',
          version: '2.3.11',
          dependencies: {
            '@biomejs/cli-linux-x64': '2.3.11',
            '@biomejs/cli-linux-x64-musl': '2.3.11',
            '@biomejs/cli-darwin-arm64': '2.3.11',
            '@biomejs/cli-darwin-x64': '2.3.11',
            '@biomejs/cli-win32-arm64': '2.3.11',
            '@biomejs/cli-win32-x64': '2.3.11',
          },
          dist: {
            tarball: 'https://registry.npmjs.org/@biomejs/biome/-/biome-2.3.11.tgz',
            integrity: 'sha512-test1234567890123456789012345678901234567890123456789012345678',
          },
        },
      },
    }

    const cliLinuxX64MetaFull = {
      name: '@biomejs/cli-linux-x64',
      'dist-tags': { latest: '2.3.11' },
      versions: {
        '2.3.11': {
          name: '@biomejs/cli-linux-x64',
          version: '2.3.11',
          os: ['linux'],
          cpu: ['x64'],
          libc: ['glibc'],
          dist: {
            tarball: 'https://registry.npmjs.org/@biomejs/cli-linux-x64/-/cli-linux-x64-2.3.11.tgz',
            integrity: 'sha512-glibc1234567890123456789012345678901234567890123456789012345678',
          },
        },
      },
    }

    const cliLinuxX64MuslMetaFull = {
      name: '@biomejs/cli-linux-x64-musl',
      'dist-tags': { latest: '2.3.11' },
      versions: {
        '2.3.11': {
          name: '@biomejs/cli-linux-x64-musl',
          version: '2.3.11',
          os: ['linux'],
          cpu: ['x64'],
          libc: ['musl'],
          dist: {
            tarball: 'https://registry.npmjs.org/@biomejs/cli-linux-x64-musl/-/cli-linux-x64-musl-2.3.11.tgz',
            integrity: 'sha512-musl1234567890123456789012345678901234567890123456789012345678',
          },
        },
      },
    }

    nock(registries.default)
      // cspell:disable-next-line
      .get('/@biomejs%2Fbiome')
      .reply(200, biomejsBiomeMetaFull)

    nock(registries.default)
      // cspell:disable-next-line
      .get('/@biomejs%2Fcli-linux-x64')
      .reply(200, cliLinuxX64MetaFull)

    nock(registries.default)
      // cspell:disable-next-line
      .get('/@biomejs%2Fcli-linux-x64-musl')
      .reply(200, cliLinuxX64MuslMetaFull)

    const cacheDir = temporaryDirectory()
    const { resolveFromNpm } = createResolveFromNpm({
      storeDir: temporaryDirectory(),
      cacheDir,
      registries,
    })

    const wantedDependency: WantedDependency = {
      alias: '@biomejs/biome',
      bareSpecifier: '2.3.11',
    }
    const resolveResult = await resolveFromNpm(
      wantedDependency,
      { optional: true }
    )

    expect(resolveResult!.id).toBe('@biomejs/biome@2.3.11')
    expect(resolveResult!.manifest!.dependencies).toBeDefined()

    expect(resolveResult!.manifest!.dependencies!['@biomejs/cli-linux-x64']).toBe('2.3.11')
    expect(resolveResult!.manifest!.dependencies!['@biomejs/cli-linux-x64-musl']).toBe('2.3.11')
  })

  test('should request full metadata for optional deps and abbreviated for regular deps', async () => {
    const testPackageMetaAbbreviated = {
      name: 'test-package',
      'dist-tags': { latest: '1.0.0' },
      versions: {
        '1.0.0': {
          name: 'test-package',
          version: '1.0.0',
          dist: {
            tarball: 'https://registry.npmjs.org/test-package/-/test-package-1.0.0.tgz',
            integrity: 'sha512-test1234567890123456789012345678901234567890123456789012345678',
          },
        },
      },
    }

    nock(registries.default)
      .get('/test-package')
      .times(2)
      .reply(200, testPackageMetaAbbreviated)

    const cacheDir = temporaryDirectory()

    {
      const { resolveFromNpm } = createResolveFromNpm({
        storeDir: temporaryDirectory(),
        cacheDir,
        registries,
      })
      const regularWantedDep: WantedDependency = {
        alias: 'test-package',
        bareSpecifier: '1.0.0',
      }
      const regularResult = await resolveFromNpm(regularWantedDep, {})
      expect(regularResult!.id).toBe('test-package@1.0.0')
      expect(regularResult!.manifest).toBeDefined()
    }

    {
      const { resolveFromNpm } = createResolveFromNpm({
        storeDir: temporaryDirectory(),
        cacheDir,
        registries,
      })
      const optionalWantedDep: WantedDependency = {
        alias: 'test-package',
        bareSpecifier: '1.0.0',
      }
      const optionalResult = await resolveFromNpm(
        optionalWantedDep,
        { optional: true }
      )
      expect(optionalResult!.id).toBe('test-package@1.0.0')
      expect(optionalResult!.manifest).toBeDefined()
    }
  })

  test('should include metaDir in cache key to separate abbreviated and full metadata', async () => {
    const cacheTestMetaAbbreviated = {
      name: 'cache-test',
      'dist-tags': { latest: '1.0.0' },
      versions: {
        '1.0.0': {
          name: 'cache-test',
          version: '1.0.0',
          dist: {
            tarball: 'https://registry.npmjs.org/cache-test/-/cache-test-1.0.0.tgz',
            integrity: 'sha512-test1234567890123456789012345678901234567890123456789012345678',
          },
        },
      },
    }
    const cacheTestMetaFull = {
      ...cacheTestMetaAbbreviated,
      versions: {
        '1.0.0': {
          ...cacheTestMetaAbbreviated.versions['1.0.0'],
          scripts: {
            test: 'jest',
          },
        },
      },
    }

    nock(registries.default)
      .get('/cache-test')
      .reply(200, cacheTestMetaAbbreviated)

    nock(registries.default)
      .get('/cache-test')
      .reply(200, cacheTestMetaFull)

    const cacheDir = temporaryDirectory()

    {
      const { resolveFromNpm } = createResolveFromNpm({
        fullMetadata: false,
        storeDir: temporaryDirectory(),
        cacheDir,
        registries,
      })
      const wantedDep: WantedDependency = {
        alias: 'cache-test',
        bareSpecifier: '1.0.0',
      }
      const result = await resolveFromNpm(wantedDep, {})
      expect(result!.manifest!.scripts).toBeFalsy()
    }

    {
      const { resolveFromNpm } = createResolveFromNpm({
        storeDir: temporaryDirectory(),
        cacheDir,
        registries,
      })
      const wantedDep: WantedDependency = {
        alias: 'cache-test',
        bareSpecifier: '1.0.0',
      }
      const result = await resolveFromNpm(
        wantedDep,
        { optional: true }
      )
      expect(result!.manifest!.scripts).toBeDefined()
      expect(result!.manifest!.scripts!.test).toBe('jest')
    }
  })
})
