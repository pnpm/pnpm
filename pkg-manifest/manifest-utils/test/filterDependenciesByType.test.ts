import { filterDependenciesByType, getAllDependenciesFromManifest } from '@pnpm/manifest-utils'

const ALL_INCLUDED = {
  dependencies: true,
  devDependencies: true,
  optionalDependencies: true,
}

describe('filterDependenciesByType', () => {
  test('includes peerDependencies when autoInstallPeers is true', () => {
    const result = filterDependenciesByType({
      peerDependencies: { foo: '>=1.0.0' },
    }, ALL_INCLUDED, { autoInstallPeers: true })

    expect(result).toEqual({ foo: '>=1.0.0' })
  })

  test('excludes peerDependencies when autoInstallPeers is false', () => {
    const result = filterDependenciesByType({
      peerDependencies: { foo: '>=1.0.0' },
    }, ALL_INCLUDED, { autoInstallPeers: false })

    expect(result).toEqual({})
  })

  test('excludes peerDependencies when opts is omitted', () => {
    const result = filterDependenciesByType({
      peerDependencies: { foo: '>=1.0.0' },
    }, ALL_INCLUDED)

    expect(result).toEqual({})
  })

  test('concrete dependencies override peer ranges', () => {
    const result = filterDependenciesByType({
      peerDependencies: { foo: '>=1.0.0' },
      dependencies: { foo: '^2.0.0' },
    }, ALL_INCLUDED, { autoInstallPeers: true })

    expect(result.foo).toBe('^2.0.0')
  })

  test('devDependencies override peer ranges', () => {
    const result = filterDependenciesByType({
      peerDependencies: { foo: '>=1.0.0' },
      devDependencies: { foo: '^3.0.0' },
    }, ALL_INCLUDED, { autoInstallPeers: true })

    expect(result.foo).toBe('^3.0.0')
  })
})

describe('getAllDependenciesFromManifest', () => {
  test('includes peerDependencies when autoInstallPeers is true', () => {
    const result = getAllDependenciesFromManifest({
      peerDependencies: { foo: '>=1.0.0' },
    }, { autoInstallPeers: true })

    expect(result).toEqual({ foo: '>=1.0.0' })
  })

  test('excludes peerDependencies when opts is omitted', () => {
    const result = getAllDependenciesFromManifest({
      peerDependencies: { foo: '>=1.0.0' },
    })

    expect(result).toEqual({})
  })

  test('concrete dependencies override peer ranges', () => {
    const result = getAllDependenciesFromManifest({
      peerDependencies: { foo: '>=1.0.0' },
      dependencies: { foo: '^2.0.0' },
    }, { autoInstallPeers: true })

    expect(result.foo).toBe('^2.0.0')
  })
})
