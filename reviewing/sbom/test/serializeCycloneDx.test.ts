import { describe, expect, it } from '@jest/globals'
import { serializeCycloneDx, type SbomResult } from '@pnpm/sbom'
import { DepType } from '@pnpm/lockfile.detect-dep-types'

function makeSbomResult (): SbomResult {
  return {
    rootComponent: {
      name: '@myorg/my-app',
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
        tarballUrl: 'https://registry.npmjs.org/lodash/-/lodash-4.17.21.tgz',
        license: 'MIT',
        description: 'Lodash modular utilities',
        author: 'Jane Doe',
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
      { from: 'pkg:npm/%40myorg/my-app@1.0.0', to: 'pkg:npm/lodash@4.17.21' },
      { from: 'pkg:npm/%40myorg/my-app@1.0.0', to: 'pkg:npm/%40babel/core@7.23.0' },
    ],
  }
}

describe('serializeCycloneDx', () => {
  it('should produce valid CycloneDX 1.7 JSON', () => {
    const result = makeSbomResult()
    const json = serializeCycloneDx(result)
    const parsed = JSON.parse(json)

    expect(parsed.bomFormat).toBe('CycloneDX')
    expect(parsed.specVersion).toBe('1.7')
    expect(parsed.version).toBe(1)
    expect(parsed.serialNumber).toMatch(/^urn:uuid:[0-9a-f-]+$/)
  })

  it('should split scoped root component into group and name', () => {
    const result = makeSbomResult()
    const parsed = JSON.parse(serializeCycloneDx(result))

    expect(parsed.metadata.component.group).toBe('@myorg')
    expect(parsed.metadata.component.name).toBe('my-app')
    expect(parsed.metadata.component.version).toBe('1.0.0')
    expect(parsed.metadata.component.type).toBe('application')
  })

  it('should split scoped component names into group and name', () => {
    const result = makeSbomResult()
    const parsed = JSON.parse(serializeCycloneDx(result))

    const babel = parsed.components[1]
    expect(babel.group).toBe('@babel')
    expect(babel.name).toBe('core')

    const lodash = parsed.components[0]
    expect(lodash.group).toBeUndefined()
    expect(lodash.name).toBe('lodash')
  })

  it('should include tools.components with versions when toolInfo is provided', () => {
    const result = makeSbomResult()
    const parsed = JSON.parse(serializeCycloneDx(result, {
      pnpmVersion: '11.0.0',
    }))

    const tools = parsed.metadata.tools.components
    expect(tools).toHaveLength(1)
    expect(tools[0]).toEqual({ type: 'application', name: 'pnpm', version: '11.0.0' })
  })

  it('should include all components with PURLs', () => {
    const result = makeSbomResult()
    const parsed = JSON.parse(serializeCycloneDx(result))

    expect(parsed.components).toHaveLength(2)
    expect(parsed.components[0].purl).toBe('pkg:npm/lodash@4.17.21')
    expect(parsed.components[0].type).toBe('library')
    expect(parsed.components[1].purl).toBe('pkg:npm/%40babel/core@7.23.0')
  })

  it('should place hashes in externalReferences with type distribution', () => {
    const result = makeSbomResult()
    const parsed = JSON.parse(serializeCycloneDx(result))

    const lodash = parsed.components[0]
    expect(lodash.hashes).toBeUndefined()

    const distRef = lodash.externalReferences.find(
      (r: { type: string }) => r.type === 'distribution'
    )
    expect(distRef).toBeDefined()
    expect(distRef.url).toBe('https://registry.npmjs.org/lodash/-/lodash-4.17.21.tgz')
    expect(distRef.hashes.length).toBeGreaterThan(0)
    expect(distRef.hashes[0].alg).toBeDefined()
    expect(distRef.hashes[0].content).toBeDefined()
  })

  it('should use license.id for known SPDX identifiers', () => {
    const result = makeSbomResult()
    const parsed = JSON.parse(serializeCycloneDx(result))

    expect(parsed.components[0].licenses).toEqual([
      { license: { id: 'MIT' } },
    ])
  })

  it('should use expression for compound SPDX licenses', () => {
    const result = makeSbomResult()
    result.components[0].license = 'MIT OR Apache-2.0'
    const parsed = JSON.parse(serializeCycloneDx(result))

    expect(parsed.components[0].licenses).toEqual([
      { expression: 'MIT OR Apache-2.0' },
    ])
  })

  it('should use license.name for non-SPDX license strings', () => {
    const result = makeSbomResult()
    result.components[0].license = 'SEE LICENSE IN LICENSE.md'
    const parsed = JSON.parse(serializeCycloneDx(result))

    expect(parsed.components[0].licenses).toEqual([
      { license: { name: 'SEE LICENSE IN LICENSE.md' } },
    ])
  })

  it('should include authors array when author is present', () => {
    const result = makeSbomResult()
    const parsed = JSON.parse(serializeCycloneDx(result))

    expect(parsed.components[0].authors).toEqual([{ name: 'Jane Doe' }])
    expect(parsed.components[1].authors).toBeUndefined()
  })

  it('should include dependencies', () => {
    const result = makeSbomResult()
    const parsed = JSON.parse(serializeCycloneDx(result))

    expect(parsed.dependencies).toBeDefined()
    const rootDep = parsed.dependencies.find(
      (d: { ref: string }) => d.ref === 'pkg:npm/%40myorg/my-app@1.0.0'
    )
    expect(rootDep).toBeDefined()
    expect(rootDep.dependsOn).toContain('pkg:npm/lodash@4.17.21')
    expect(rootDep.dependsOn).toContain('pkg:npm/%40babel/core@7.23.0')
  })
})
