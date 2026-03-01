import { describe, expect, it } from '@jest/globals'
import { serializeCycloneDx, type SbomResult } from '@pnpm/sbom'
import { DepType } from '@pnpm/lockfile.detect-dep-types'

function makeSbomResult (): SbomResult {
  return {
    rootComponent: {
      name: '@acme/sbom-app',
      version: '1.0.0',
      type: 'application',
      license: 'MIT',
      description: 'ACME SBOM application',
      author: 'ACME Corp',
      repository: 'https://github.com/acme/sbom-app.git',
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
        repository: 'https://github.com/lodash/lodash.git',
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
      { from: 'pkg:npm/%40acme/sbom-app@1.0.0', to: 'pkg:npm/lodash@4.17.21' },
      { from: 'pkg:npm/%40acme/sbom-app@1.0.0', to: 'pkg:npm/%40babel/core@7.23.0' },
    ],
  }
}

describe('serializeCycloneDx', () => {
  it('should produce valid CycloneDX 1.7 JSON', () => {
    const result = makeSbomResult()
    const json = serializeCycloneDx(result)
    const parsed = JSON.parse(json)

    expect(parsed.$schema).toBe('http://cyclonedx.org/schema/bom-1.7.schema.json')
    expect(parsed.bomFormat).toBe('CycloneDX')
    expect(parsed.specVersion).toBe('1.7')
    expect(parsed.version).toBe(1)
    expect(parsed.serialNumber).toMatch(/^urn:uuid:[0-9a-f-]+$/)
  })

  it('should include timestamp in metadata', () => {
    const before = new Date().toISOString()
    const result = makeSbomResult()
    const parsed = JSON.parse(serializeCycloneDx(result))
    const after = new Date().toISOString()

    expect(parsed.metadata.timestamp).toBeDefined()
    expect(parsed.metadata.timestamp >= before).toBe(true)
    expect(parsed.metadata.timestamp <= after).toBe(true)
  })

  it('should use build lifecycle by default', () => {
    const result = makeSbomResult()
    const parsed = JSON.parse(serializeCycloneDx(result))

    expect(parsed.metadata.lifecycles).toEqual([{ phase: 'build' }])
  })

  it('should use pre-build lifecycle when lockfileOnly is true', () => {
    const result = makeSbomResult()
    const parsed = JSON.parse(serializeCycloneDx(result, { lockfileOnly: true }))

    expect(parsed.metadata.lifecycles).toEqual([{ phase: 'pre-build' }])
  })

  it('should split scoped root component into group and name', () => {
    const result = makeSbomResult()
    const parsed = JSON.parse(serializeCycloneDx(result))

    expect(parsed.metadata.component.group).toBe('@acme')
    expect(parsed.metadata.component.name).toBe('sbom-app')
    expect(parsed.metadata.component.version).toBe('1.0.0')
    expect(parsed.metadata.component.type).toBe('application')
    expect(parsed.metadata.component.purl).toBe('pkg:npm/%40acme/sbom-app@1.0.0')
  })

  it('should include root component metadata', () => {
    const result = makeSbomResult()
    const parsed = JSON.parse(serializeCycloneDx(result))

    const root = parsed.metadata.component
    expect(root.licenses).toEqual([{ license: { id: 'MIT' } }])
    expect(root.description).toBe('ACME SBOM application')
    expect(root.authors).toEqual([{ name: 'ACME Corp' }])
    expect(root.supplier).toBeUndefined()
    const vcsRef = root.externalReferences.find(
      (r: { type: string }) => r.type === 'vcs'
    )
    expect(vcsRef.url).toBe('https://github.com/acme/sbom-app.git')
  })

  it('should not include metadata.authors or metadata.supplier by default', () => {
    const result = makeSbomResult()
    const parsed = JSON.parse(serializeCycloneDx(result))

    expect(parsed.metadata.authors).toBeUndefined()
    expect(parsed.metadata.supplier).toBeUndefined()
  })

  it('should include metadata.authors and metadata.supplier when provided via options', () => {
    const result = makeSbomResult()
    const parsed = JSON.parse(serializeCycloneDx(result, {
      sbomAuthors: ['Jane Doe', 'John Smith'],
      sbomSupplier: 'ACME Corp',
    }))

    expect(parsed.metadata.authors).toEqual([{ name: 'Jane Doe' }, { name: 'John Smith' }])
    expect(parsed.metadata.supplier).toEqual({ name: 'ACME Corp' })
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

  it('should include component authors without supplier', () => {
    const result = makeSbomResult()
    const parsed = JSON.parse(serializeCycloneDx(result))

    expect(parsed.components[0].authors).toEqual([{ name: 'Jane Doe' }])
    expect(parsed.components[0].supplier).toBeUndefined()
    expect(parsed.components[1].authors).toBeUndefined()
  })

  it('should include vcs externalReference when repository is present', () => {
    const result = makeSbomResult()
    const parsed = JSON.parse(serializeCycloneDx(result))

    const lodash = parsed.components[0]
    const vcsRef = lodash.externalReferences.find(
      (r: { type: string }) => r.type === 'vcs'
    )
    expect(vcsRef).toBeDefined()
    expect(vcsRef.url).toBe('https://github.com/lodash/lodash.git')

    const babel = parsed.components[1]
    expect(babel.externalReferences).toBeUndefined()
  })

  it('should include dependencies', () => {
    const result = makeSbomResult()
    const parsed = JSON.parse(serializeCycloneDx(result))

    expect(parsed.dependencies).toBeDefined()
    const rootDep = parsed.dependencies.find(
      (d: { ref: string }) => d.ref === 'pkg:npm/%40acme/sbom-app@1.0.0'
    )
    expect(rootDep).toBeDefined()
    expect(rootDep.dependsOn).toContain('pkg:npm/lodash@4.17.21')
    expect(rootDep.dependsOn).toContain('pkg:npm/%40babel/core@7.23.0')
  })
})
