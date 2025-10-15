import { filterPkgMetadataByPublishDate, clearLoggedPackages } from '@pnpm/registry.pkg-metadata-filter'
import * as logger from '@pnpm/logger'

test('filterPkgMetadataByPublishDate', () => {
  const cutoff = new Date('2020-04-01T00:00:00.000Z')
  const name = 'dist-tag-date'
  expect(filterPkgMetadataByPublishDate({
    name,
    versions: {
      '3.0.0': {
        name,
        version: '3.0.0',
        dist: { tarball: `https://registry.npmjs.org/${name}/-/${name}-3.0.0.tgz`, shasum: '' },
      },
      '3.1.0': {
        name,
        version: '3.1.0',
        dist: { tarball: `https://registry.npmjs.org/${name}/-/${name}-3.1.0.tgz`, shasum: '' },
        deprecated: 'This version is deprecated',
      },
      '3.2.0': {
        name,
        version: '3.2.0',
        dist: { tarball: `https://registry.npmjs.org/${name}/-/${name}-3.2.0.tgz`, shasum: '' },
      },
      '2.9.9': {
        name,
        version: '2.9.9',
        dist: { tarball: `https://registry.npmjs.org/${name}/-/${name}-2.9.9.tgz`, shasum: '' },
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
  }, cutoff)).toMatchSnapshot()
})

test('filterPkgMetadataByPublishDate logs skipped versions', () => {
  clearLoggedPackages()
  const cutoff = new Date('2020-04-01T00:00:00.000Z')
  const name = 'test-package'
  const infoMessages: string[] = []
  jest.spyOn(logger, 'globalInfo').mockImplementation((msg: string) => {
    infoMessages.push(msg)
  })

  filterPkgMetadataByPublishDate({
    name,
    versions: {
      '1.0.0': {
        name,
        version: '1.0.0',
        dist: { tarball: `https://registry.npmjs.org/${name}/-/${name}-1.0.0.tgz`, shasum: '' },
      },
      '1.1.0': {
        name,
        version: '1.1.0',
        dist: { tarball: `https://registry.npmjs.org/${name}/-/${name}-1.1.0.tgz`, shasum: '' },
      },
      '1.2.0': {
        name,
        version: '1.2.0',
        dist: { tarball: `https://registry.npmjs.org/${name}/-/${name}-1.2.0.tgz`, shasum: '' },
      },
    },
    'dist-tags': {
      latest: '1.2.0',
    },
    time: {
      '1.0.0': '2020-01-01T00:00:00.000Z',
      '1.1.0': '2020-03-01T00:00:00.000Z',
      '1.2.0': '2020-05-01T00:00:00.000Z', // This will be skipped
    },
  }, cutoff)

  expect(infoMessages).toHaveLength(1)
  expect(infoMessages[0]).toBe('test-package: Skipping version(s) due to minimumReleaseAge: 1.2.0')

  jest.restoreAllMocks()
})
