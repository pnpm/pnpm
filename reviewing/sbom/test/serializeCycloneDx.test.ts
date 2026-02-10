import { describe, expect, it } from '@jest/globals'
import { serializeCycloneDx, type SbomResult } from '@pnpm/sbom'
import { DepType } from '@pnpm/lockfile.detect-dep-types'

function makeSbomResult (): SbomResult {
  return {
    rootComponent: {
      name: 'my-app',
      version: '1.0.0',
      type: 'application',
    },
    components: [
      {
        name: 'lodash',
        version: '4.17.21',
        purl: 'pkg:npm/lodash@4.17.21',
        depPath: 'lodash@4.17.21',
        depType: DepType.ProdOnly,
        integrity: 'sha512-LCt5klFGBqVfMfB1GL1o2Ll+0w/DeN2OZGR8U2/9fns=',
        license: 'MIT',
        description: 'Lodash modular utilities',
        author: 'John-David Dalton',
        homepage: 'https://lodash.com/',
      },
      {
        name: '@babel/core',
        version: '7.23.0',
        purl: 'pkg:npm/%40babel/core@7.23.0',
        depPath: '@babel/core@7.23.0',
        depType: DepType.DevOnly,
        license: 'MIT',
      },
    ],
    relationships: [
      { from: 'pkg:npm/my-app@1.0.0', to: 'pkg:npm/lodash@4.17.21' },
      { from: 'pkg:npm/my-app@1.0.0', to: 'pkg:npm/%40babel/core@7.23.0' },
    ],
  }
}

describe('serializeCycloneDx', () => {
  it('should produce valid CycloneDX 1.6 JSON', () => {
    const result = makeSbomResult()
    const json = serializeCycloneDx(result)
    const parsed = JSON.parse(json)

    expect(parsed.bomFormat).toBe('CycloneDX')
    expect(parsed.specVersion).toBe('1.6')
    expect(parsed.version).toBe(1)
    expect(parsed.serialNumber).toMatch(/^urn:uuid:[0-9a-f-]+$/)
  })

  it('should include metadata with tool and root component', () => {
    const result = makeSbomResult()
    const parsed = JSON.parse(serializeCycloneDx(result))

    expect(parsed.metadata.tools).toEqual([{ name: 'pnpm' }])
    expect(parsed.metadata.component.name).toBe('my-app')
    expect(parsed.metadata.component.version).toBe('1.0.0')
    expect(parsed.metadata.component.type).toBe('application')
  })

  it('should include all components with PURLs', () => {
    const result = makeSbomResult()
    const parsed = JSON.parse(serializeCycloneDx(result))

    expect(parsed.components).toHaveLength(2)
    expect(parsed.components[0].purl).toBe('pkg:npm/lodash@4.17.21')
    expect(parsed.components[0].type).toBe('library')
    expect(parsed.components[1].purl).toBe('pkg:npm/%40babel/core@7.23.0')
  })

  it('should include hashes when integrity is present', () => {
    const result = makeSbomResult()
    const parsed = JSON.parse(serializeCycloneDx(result))

    const lodash = parsed.components[0]
    expect(lodash.hashes).toBeDefined()
    expect(lodash.hashes.length).toBeGreaterThan(0)
    expect(lodash.hashes[0].alg).toBeDefined()
    expect(lodash.hashes[0].content).toBeDefined()
  })

  it('should include license info', () => {
    const result = makeSbomResult()
    const parsed = JSON.parse(serializeCycloneDx(result))

    expect(parsed.components[0].licenses).toEqual([
      { license: { name: 'MIT' } },
    ])
  })

  it('should include authors array when author is present', () => {
    const result = makeSbomResult()
    const parsed = JSON.parse(serializeCycloneDx(result))

    expect(parsed.components[0].authors).toEqual([{ name: 'John-David Dalton' }])
    expect(parsed.components[1].authors).toBeUndefined()
  })

  it('should include dependencies', () => {
    const result = makeSbomResult()
    const parsed = JSON.parse(serializeCycloneDx(result))

    expect(parsed.dependencies).toBeDefined()
    const rootDep = parsed.dependencies.find(
      (d: { ref: string }) => d.ref === 'pkg:npm/my-app@1.0.0'
    )
    expect(rootDep).toBeDefined()
    expect(rootDep.dependsOn).toContain('pkg:npm/lodash@4.17.21')
    expect(rootDep.dependsOn).toContain('pkg:npm/%40babel/core@7.23.0')
  })
})
