import { normalizeConfigDeps } from '@pnpm/config.deps-installer'
import { getIntegrity, REGISTRY_MOCK_PORT } from '@pnpm/registry-mock'

const registry = `http://localhost:${REGISTRY_MOCK_PORT}/`

test('normalizes string spec with integrity to structured dependency and computes tarball', () => {
  const deps = normalizeConfigDeps({
    '@pnpm.e2e/foo': `100.0.0+${getIntegrity('@pnpm.e2e/foo', '100.0.0')}`,
  }, {
    registries: {
      default: registry,
    },
  })
  expect(deps['@pnpm.e2e/foo']).toStrictEqual({
    version: '100.0.0',
    resolution: {
      integrity: getIntegrity('@pnpm.e2e/foo', '100.0.0'),
      tarball: `${registry}@pnpm.e2e/foo/-/foo-100.0.0.tgz`,
    },
  })
})

test('keeps provided tarball when resolution.tarball is specified', () => {
  const customTarball = 'https://custom.example.com/foo-100.0.0.tgz'
  const deps = normalizeConfigDeps({
    '@pnpm.e2e/foo': {
      integrity: `100.0.0+${getIntegrity('@pnpm.e2e/foo', '100.0.0')}`,
      tarball: customTarball,
    },
  }, {
    registries: {
      default: registry,
    },
  })
  expect(deps['@pnpm.e2e/foo']).toStrictEqual({
    version: '100.0.0',
    resolution: {
      integrity: getIntegrity('@pnpm.e2e/foo', '100.0.0'),
      tarball: customTarball,
    },
  })
})

test('computes tarball when not specified in object spec', () => {
  const deps = normalizeConfigDeps({
    '@pnpm.e2e/foo': {
      integrity: `100.0.0+${getIntegrity('@pnpm.e2e/foo', '100.0.0')}`,
    },
  }, {
    registries: {
      default: registry,
    },
  })
  expect(deps['@pnpm.e2e/foo']).toStrictEqual({
    version: '100.0.0',
    resolution: {
      integrity: getIntegrity('@pnpm.e2e/foo', '100.0.0'),
      tarball: `${registry}@pnpm.e2e/foo/-/foo-100.0.0.tgz`,
    },
  })
})

test('throws when string spec does not include integrity', () => {
  expect(() => normalizeConfigDeps({
    '@pnpm.e2e/foo': '100.0.0',
  }, {
    registries: {
      default: registry,
    },
  })).toThrow("doesn't have an integrity checksum")
})

test('throws when object spec does not include integrity', () => {
  expect(() => normalizeConfigDeps({
    '@pnpm.e2e/foo': {
      integrity: '100.0.0',
    },
  }, {
    registries: {
      default: registry,
    },
  })).toThrow("doesn't have an integrity checksum")
})
