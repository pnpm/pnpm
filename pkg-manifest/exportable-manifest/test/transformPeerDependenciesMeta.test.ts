import { type ProjectManifest } from '@pnpm/types'
import { transformPeerDependenciesMeta } from '../lib/transform/peerDependenciesMeta.js'

test('returns manifest as-is when peerDependenciesMeta is absent', () => {
  const manifest: ProjectManifest = {
    name: 'foo',
    version: '1.0.0',
  }
  expect(transformPeerDependenciesMeta(manifest)).toStrictEqual(manifest)
})

test('returns manifest as-is when peerDependenciesMeta is undefined', () => {
  const manifest: ProjectManifest = {
    name: 'foo',
    version: '1.0.0',
    peerDependenciesMeta: undefined,
  }
  expect(transformPeerDependenciesMeta(manifest)).toStrictEqual({
    name: 'foo',
    version: '1.0.0',
    peerDependenciesMeta: undefined,
  })
})

test('defaults optional to false when not specified', () => {
  const manifest: ProjectManifest = {
    name: 'foo',
    version: '1.0.0',
    peerDependenciesMeta: {
      bar: {},
    },
  }
  expect(transformPeerDependenciesMeta(manifest)).toStrictEqual({
    name: 'foo',
    version: '1.0.0',
    peerDependenciesMeta: {
      bar: {
        optional: false,
      },
    },
  })
})

test('preserves optional when explicitly set to false', () => {
  const manifest: ProjectManifest = {
    name: 'foo',
    version: '1.0.0',
    peerDependenciesMeta: {
      bar: {
        optional: false,
      },
    },
  }
  expect(transformPeerDependenciesMeta(manifest)).toStrictEqual({
    name: 'foo',
    version: '1.0.0',
    peerDependenciesMeta: {
      bar: {
        optional: false,
      },
    },
  })
})

test('preserves optional when explicitly set to true', () => {
  const manifest: ProjectManifest = {
    name: 'foo',
    version: '1.0.0',
    peerDependenciesMeta: {
      bar: {
        optional: true,
      },
    },
  }
  expect(transformPeerDependenciesMeta(manifest)).toStrictEqual({
    name: 'foo',
    version: '1.0.0',
    peerDependenciesMeta: {
      bar: {
        optional: true,
      },
    },
  })
})

test('handles multiple peerDependenciesMeta entries with different values', () => {
  const manifest: ProjectManifest = {
    name: 'foo',
    version: '1.0.0',
    peerDependenciesMeta: {
      bar: {
        optional: true,
      },
      baz: {
        optional: false,
      },
      qux: {},
    },
  }
  expect(transformPeerDependenciesMeta(manifest)).toStrictEqual({
    name: 'foo',
    version: '1.0.0',
    peerDependenciesMeta: {
      bar: {
        optional: true,
      },
      baz: {
        optional: false,
      },
      qux: {
        optional: false,
      },
    },
  })
})

test('preserves additional properties in peerDependenciesMeta', () => {
  const manifest: ProjectManifest = {
    name: 'foo',
    version: '1.0.0',
    peerDependenciesMeta: {
      bar: {
        optional: true,
        // @ts-expect-error - testing non-standard properties
        customProp: 'value',
      },
    },
  }
  expect(transformPeerDependenciesMeta(manifest)).toStrictEqual({
    name: 'foo',
    version: '1.0.0',
    peerDependenciesMeta: {
      bar: {
        customProp: 'value',
        optional: true,
      },
    },
  })
})

test('preserves other manifest properties', () => {
  const manifest: ProjectManifest = {
    name: 'foo',
    version: '1.0.0',
    description: 'A test package',
    dependencies: {
      lodash: '^4.0.0',
    },
    peerDependenciesMeta: {
      react: {
        optional: true,
      },
    },
  }
  expect(transformPeerDependenciesMeta(manifest)).toStrictEqual({
    name: 'foo',
    version: '1.0.0',
    description: 'A test package',
    dependencies: {
      lodash: '^4.0.0',
    },
    peerDependenciesMeta: {
      react: {
        optional: true,
      },
    },
  })
})
