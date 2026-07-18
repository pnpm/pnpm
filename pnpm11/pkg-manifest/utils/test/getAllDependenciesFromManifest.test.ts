import { expect, test } from '@jest/globals'
import { getAllDependenciesFromManifest } from '@pnpm/pkg-manifest.utils'

test('getAllDependenciesFromManifest() merges all dependency fields', () => {
  expect(getAllDependenciesFromManifest({
    devDependencies: {
      foo: '1.0.0',
    },
    dependencies: {
      bar: '^2.0.0',
    },
    optionalDependencies: {
      qar: '~3.0.0',
    },
  })).toStrictEqual({
    foo: '1.0.0',
    bar: '^2.0.0',
    qar: '~3.0.0',
  })
})

test('getAllDependenciesFromManifest() ignores peerDependencies by default', () => {
  expect(getAllDependenciesFromManifest({
    dependencies: {
      bar: '^2.0.0',
    },
    peerDependencies: {
      react: '^19.0.0',
    },
  })).toStrictEqual({
    bar: '^2.0.0',
  })
})

test('getAllDependenciesFromManifest() includes peerDependencies when autoInstallPeers is true', () => {
  expect(getAllDependenciesFromManifest({
    dependencies: {
      bar: '^2.0.0',
    },
    peerDependencies: {
      react: '^19.0.0',
    },
  }, { autoInstallPeers: true })).toStrictEqual({
    bar: '^2.0.0',
    react: '^19.0.0',
  })
})

// https://github.com/pnpm/pnpm/issues/13108
test('getAllDependenciesFromManifest() does not let peerDependencies override the specifiers of installed dependencies', () => {
  expect(getAllDependenciesFromManifest({
    devDependencies: {
      react: '19.0.0',
    },
    peerDependencies: {
      react: '^19.0.0',
    },
  }, { autoInstallPeers: true })).toStrictEqual({
    react: '19.0.0',
  })
  expect(getAllDependenciesFromManifest({
    dependencies: {
      react: '~19.0.0',
    },
    peerDependencies: {
      react: '^19.0.0',
    },
  }, { autoInstallPeers: true })).toStrictEqual({
    react: '~19.0.0',
  })
  expect(getAllDependenciesFromManifest({
    optionalDependencies: {
      react: '19.0.0',
    },
    peerDependencies: {
      react: '^19.0.0',
    },
  }, { autoInstallPeers: true })).toStrictEqual({
    react: '19.0.0',
  })
})
