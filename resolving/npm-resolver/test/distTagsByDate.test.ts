import { createFetchFromRegistry } from '@pnpm/fetch'
import { createNpmResolver } from '@pnpm/npm-resolver'
import { type Registries } from '@pnpm/types'
import nock from 'nock'
import { temporaryDirectory } from 'tempy'

const registries: Registries = {
  default: 'https://registry.npmjs.org/',
}

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

test('repopulate dist-tag to highest same-major version within the date cutoff', async () => {
  const name = 'dist-tag-date'
  const meta = {
    name,
    versions: {
      '3.0.0': {
        name,
        version: '3.0.0',
        dist: { tarball: `https://registry.npmjs.org/${name}/-/${name}-3.0.0.tgz` },
      },
      '3.1.0': {
        name,
        version: '3.1.0',
        dist: { tarball: `https://registry.npmjs.org/${name}/-/${name}-3.1.0.tgz` },
      },
      '3.2.0': {
        name,
        version: '3.2.0',
        dist: { tarball: `https://registry.npmjs.org/${name}/-/${name}-3.2.0.tgz` },
      },
      '2.9.9': {
        name,
        version: '2.9.9',
        dist: { tarball: `https://registry.npmjs.org/${name}/-/${name}-2.9.9.tgz` },
      },
    },
    'dist-tags': {
      latest: '3.2.0',
    },
    time: {
      '2.9.9': '2020-01-01T00:00:00.000Z',
      '3.0.0': '2020-02-01T00:00:00.000Z',
      '3.1.0': '2020-03-01T00:00:00.000Z',
      '3.2.0': '2020-05-01T00:00:00.000Z',
    },
  }

  // Cutoff before 3.2.0, so latest must be remapped to 3.1.0 (same major 3)
  const cutoff = new Date('2020-04-01T00:00:00.000Z')

  nock(registries.default)
    .get(`/${name}`)
    .reply(200, meta)

  const cacheDir = temporaryDirectory()
  const { resolveFromNpm } = createResolveFromNpm({
    cacheDir,
    fullMetadata: true,
    registries,
  })

  const res = await resolveFromNpm({ alias: name, bareSpecifier: 'latest' }, {
    publishedBy: cutoff,
  })

  expect(res!.id).toBe(`${name}@3.1.0`)
})

test('repopulate dist-tag to highest same-major version within the date cutoff. Prefer non-deprecated version', async () => {
  const name = 'dist-tag-date'
  const meta = {
    name,
    versions: {
      '3.0.0': {
        name,
        version: '3.0.0',
        dist: { tarball: `https://registry.npmjs.org/${name}/-/${name}-3.0.0.tgz` },
      },
      '3.1.0': {
        name,
        version: '3.1.0',
        dist: { tarball: `https://registry.npmjs.org/${name}/-/${name}-3.1.0.tgz` },
        deprecated: 'This version is deprecated',
      },
      '3.2.0': {
        name,
        version: '3.2.0',
        dist: { tarball: `https://registry.npmjs.org/${name}/-/${name}-3.2.0.tgz` },
      },
      '2.9.9': {
        name,
        version: '2.9.9',
        dist: { tarball: `https://registry.npmjs.org/${name}/-/${name}-2.9.9.tgz` },
      },
    },
    'dist-tags': {
      latest: '3.2.0',
    },
    time: {
      '2.9.9': '2020-01-01T00:00:00.000Z',
      '3.0.0': '2020-02-01T00:00:00.000Z',
      '3.1.0': '2020-03-01T00:00:00.000Z',
      '3.2.0': '2020-05-01T00:00:00.000Z',
    },
  }

  // Cutoff before 3.2.0, so latest must be remapped to 3.1.0 (same major 3)
  const cutoff = new Date('2020-04-01T00:00:00.000Z')

  nock(registries.default)
    .get(`/${name}`)
    .reply(200, meta)

  const cacheDir = temporaryDirectory()
  const { resolveFromNpm } = createResolveFromNpm({
    cacheDir,
    fullMetadata: true,
    registries,
  })

  const res = await resolveFromNpm({ alias: name, bareSpecifier: 'latest' }, {
    publishedBy: cutoff,
  })

  expect(res!.id).toBe(`${name}@3.0.0`)
})

test('repopulate dist-tag to highest non-prerelease same-major version within the date cutoff', async () => {
  const name = 'dist-tag-date'
  const meta = {
    name,
    versions: {
      '3.0.0': {
        name,
        version: '3.0.0',
        dist: { tarball: `https://registry.npmjs.org/${name}/-/${name}-3.0.0.tgz` },
      },
      '3.1.0-alpha.0': {
        name,
        version: '3.1.0-alpha.0',
        dist: { tarball: `https://registry.npmjs.org/${name}/-/${name}-3.1.0-alpha.0.tgz` },
      },
      '3.2.0': {
        name,
        version: '3.2.0',
        dist: { tarball: `https://registry.npmjs.org/${name}/-/${name}-3.2.0.tgz` },
      },
      '2.9.9': {
        name,
        version: '2.9.9',
        dist: { tarball: `https://registry.npmjs.org/${name}/-/${name}-2.9.9.tgz` },
      },
    },
    'dist-tags': {
      latest: '3.2.0',
    },
    time: {
      '2.9.9': '2020-01-01T00:00:00.000Z',
      '3.0.0': '2020-02-01T00:00:00.000Z',
      '3.1.0-alpha.0': '2020-03-01T00:00:00.000Z',
      '3.2.0': '2020-05-01T00:00:00.000Z',
    },
  }

  // Cutoff before 3.2.0, so latest must be remapped to 3.1.0 (same major 3)
  const cutoff = new Date('2020-04-01T00:00:00.000Z')

  nock(registries.default)
    .get(`/${name}`)
    .reply(200, meta)

  const cacheDir = temporaryDirectory()
  const { resolveFromNpm } = createResolveFromNpm({
    cacheDir,
    fullMetadata: true,
    registries,
  })

  const res = await resolveFromNpm({ alias: name, bareSpecifier: 'latest' }, {
    publishedBy: cutoff,
  })

  expect(res!.id).toBe(`${name}@3.0.0`)
})

test('repopulate dist-tag to highest prerelease same-major version within the date cutoff', async () => {
  const name = 'dist-tag-date'
  const meta = {
    name,
    versions: {
      '3.0.0-alpha.0': {
        name,
        version: '3.0.0-alpha.0',
        dist: { tarball: `https://registry.npmjs.org/${name}/-/${name}-3.0.0-alpha.0.tgz` },
      },
      '3.0.0-alpha.1': {
        name,
        version: '3.0.0-alpha.1',
        dist: { tarball: `https://registry.npmjs.org/${name}/-/${name}-3.0.0-alpha.1.tgz` },
      },
      '3.0.0-alpha.2': {
        name,
        version: '3.0.0-alpha.2',
        dist: { tarball: `https://registry.npmjs.org/${name}/-/${name}-3.0.0-alpha.2.tgz` },
      },
      '3.2.0': {
        name,
        version: '3.2.0',
        dist: { tarball: `https://registry.npmjs.org/${name}/-/${name}-3.2.0.tgz` },
      },
      '2.9.9': {
        name,
        version: '2.9.9',
        dist: { tarball: `https://registry.npmjs.org/${name}/-/${name}-2.9.9.tgz` },
      },
    },
    'dist-tags': {
      latest: '3.0.0-alpha.2',
    },
    time: {
      '2.9.9': '2020-01-01T00:00:00.000Z',
      '3.0.0-alpha.0': '2020-02-01T00:00:00.000Z',
      '3.0.0-alpha.1': '2020-03-01T00:00:00.000Z',
      '3.0.0-alpha.2': '2020-05-01T00:00:00.000Z',
      '3.2.0': '2020-05-01T00:00:00.000Z',
    },
  }

  // Cutoff before 3.2.0 and 3.0.0-alpha.2, so latest must be remapped to 3.0.0-alpha.1 (the highest prerelease version within the cutoff)
  const cutoff = new Date('2020-04-01T00:00:00.000Z')

  nock(registries.default)
    .get(`/${name}`)
    .reply(200, meta)

  const cacheDir = temporaryDirectory()
  const { resolveFromNpm } = createResolveFromNpm({
    cacheDir,
    fullMetadata: true,
    registries,
  })

  const res = await resolveFromNpm({ alias: name, bareSpecifier: 'latest' }, {
    publishedBy: cutoff,
  })

  expect(res!.id).toBe(`${name}@3.0.0-alpha.1`)
})

test('keep dist-tag if original version is within the date cutoff', async () => {
  const name = 'dist-tag-date-keep'
  const meta = {
    name,
    versions: {
      '1.0.0': {
        name,
        version: '1.0.0',
        dist: { tarball: `https://registry.npmjs.org/${name}/-/${name}-1.0.0.tgz` },
      },
    },
    'dist-tags': { latest: '1.0.0' },
    time: { '1.0.0': '2020-01-01T00:00:00.000Z' },
  }

  const cutoff = new Date('2020-02-01T00:00:00.000Z')

  nock(registries.default)
    .get(`/${name}`)
    .reply(200, meta)

  const cacheDir = temporaryDirectory()
  const { resolveFromNpm } = createResolveFromNpm({
    cacheDir,
    fullMetadata: true,
    registries,
  })

  const res = await resolveFromNpm({ alias: name, bareSpecifier: 'latest' }, {
    publishedBy: cutoff,
  })

  expect(res!.id).toBe(`${name}@1.0.0`)
})
