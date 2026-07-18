import { expect, test } from '@jest/globals'
import { getAllDependenciesFromManifest } from '@pnpm/pkg-manifest.utils'

test('getAllDependenciesFromManifest() lets devDependencies win over a same-named peerDependencies entry', () => {
  expect(getAllDependenciesFromManifest({
    devDependencies: { react: '19.0.0' },
    peerDependencies: { react: '^19.0.0' },
  }, { autoInstallPeers: true })).toStrictEqual({ react: '19.0.0' })
})

test('getAllDependenciesFromManifest() lets dependencies win over a same-named peerDependencies entry', () => {
  expect(getAllDependenciesFromManifest({
    dependencies: { react: '19.0.0' },
    peerDependencies: { react: '^19.0.0' },
  }, { autoInstallPeers: true })).toStrictEqual({ react: '19.0.0' })
})

test('getAllDependenciesFromManifest() still includes a peer-only dependency when autoInstallPeers is true', () => {
  expect(getAllDependenciesFromManifest({
    peerDependencies: { react: '^19.0.0' },
  }, { autoInstallPeers: true })).toStrictEqual({ react: '^19.0.0' })
})

test('getAllDependenciesFromManifest() excludes peerDependencies when autoInstallPeers is false', () => {
  expect(getAllDependenciesFromManifest({
    devDependencies: { react: '19.0.0' },
    peerDependencies: { react: '^19.0.0' },
  }, { autoInstallPeers: false })).toStrictEqual({ react: '19.0.0' })
})

test('getAllDependenciesFromManifest() lets optionalDependencies win over a same-named peerDependencies entry', () => {
  expect(getAllDependenciesFromManifest({
    optionalDependencies: { react: '~19.0.0' },
    peerDependencies: { react: '^19.0.0' },
  }, { autoInstallPeers: true })).toStrictEqual({ react: '~19.0.0' })
})

