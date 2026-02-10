import { describe, expect, it } from '@jest/globals'
import { serializeSpdx, type SbomResult } from '@pnpm/sbom'
import { DepType } from '@pnpm/lockfile.detect-dep-types'

function makeSbomResult (): SbomResult {
  return {
    rootComponent: {
      name: 'my-app',
      version: '1.0.0',
      type: 'library',
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
        homepage: 'https://lodash.com/',
        author: 'John-David Dalton',
      },
    ],
    relationships: [
      { from: 'pkg:npm/my-app@1.0.0', to: 'pkg:npm/lodash@4.17.21' },
    ],
  }
}

describe('serializeSpdx', () => {
  it('should produce valid SPDX 2.3 JSON', () => {
    const result = makeSbomResult()
    const json = serializeSpdx(result)
    const parsed = JSON.parse(json)

    expect(parsed.spdxVersion).toBe('SPDX-2.3')
    expect(parsed.dataLicense).toBe('CC0-1.0')
    expect(parsed.SPDXID).toBe('SPDXRef-DOCUMENT')
  })

  it('should include creation info', () => {
    const result = makeSbomResult()
    const parsed = JSON.parse(serializeSpdx(result))

    expect(parsed.creationInfo).toBeDefined()
    expect(parsed.creationInfo.creators).toContain('Tool: pnpm')
    expect(parsed.creationInfo.created).toBeDefined()
  })

  it('should include root package and dependency packages', () => {
    const result = makeSbomResult()
    const parsed = JSON.parse(serializeSpdx(result))

    expect(parsed.packages).toHaveLength(2)
    expect(parsed.packages[0].SPDXID).toBe('SPDXRef-RootPackage')
    expect(parsed.packages[0].name).toBe('my-app')
  })

  it('should include PURL as external ref', () => {
    const result = makeSbomResult()
    const parsed = JSON.parse(serializeSpdx(result))

    const lodashPkg = parsed.packages[1]
    expect(lodashPkg.externalRefs).toBeDefined()
    expect(lodashPkg.externalRefs[0].referenceType).toBe('purl')
    expect(lodashPkg.externalRefs[0].referenceLocator).toBe('pkg:npm/lodash@4.17.21')
  })

  it('should include license info', () => {
    const result = makeSbomResult()
    const parsed = JSON.parse(serializeSpdx(result))

    const lodashPkg = parsed.packages[1]
    expect(lodashPkg.licenseConcluded).toBe('MIT')
    expect(lodashPkg.licenseDeclared).toBe('MIT')
  })

  it('should use NOASSERTION for missing license', () => {
    const result = makeSbomResult()
    result.components[0].license = undefined
    const parsed = JSON.parse(serializeSpdx(result))

    const lodashPkg = parsed.packages[1]
    expect(lodashPkg.licenseConcluded).toBe('NOASSERTION')
    expect(lodashPkg.licenseDeclared).toBe('NOASSERTION')
  })

  it('should include DESCRIBES relationship from document to root', () => {
    const result = makeSbomResult()
    const parsed = JSON.parse(serializeSpdx(result))

    const describes = parsed.relationships.find(
      (r: { relationshipType: string }) => r.relationshipType === 'DESCRIBES'
    )
    expect(describes).toBeDefined()
    expect(describes.spdxElementId).toBe('SPDXRef-DOCUMENT')
    expect(describes.relatedSpdxElement).toBe('SPDXRef-RootPackage')
  })

  it('should include DEPENDS_ON relationships', () => {
    const result = makeSbomResult()
    const parsed = JSON.parse(serializeSpdx(result))

    const dependsOn = parsed.relationships.filter(
      (r: { relationshipType: string }) => r.relationshipType === 'DEPENDS_ON'
    )
    expect(dependsOn).toHaveLength(1)
    expect(dependsOn[0].spdxElementId).toBe('SPDXRef-RootPackage')
  })

  it('should sanitize SPDX IDs', () => {
    const result = makeSbomResult()
    result.components[0].name = '@scope/pkg-name'
    const parsed = JSON.parse(serializeSpdx(result))

    const pkg = parsed.packages[1]
    // SPDX IDs can only contain [a-zA-Z0-9.-]
    expect(pkg.SPDXID).toMatch(/^SPDXRef-[a-zA-Z0-9.-]+$/)
  })

  it('should include document namespace', () => {
    const result = makeSbomResult()
    const parsed = JSON.parse(serializeSpdx(result))

    expect(parsed.documentNamespace).toMatch(/^https:\/\/spdx\.org\/spdxdocs\//)
  })

  it('should include description when present', () => {
    const result = makeSbomResult()
    const parsed = JSON.parse(serializeSpdx(result))

    expect(parsed.packages[1].description).toBe('Lodash modular utilities')
  })

  it('should include homepage when present', () => {
    const result = makeSbomResult()
    const parsed = JSON.parse(serializeSpdx(result))

    expect(parsed.packages[1].homepage).toBe('https://lodash.com/')
  })

  it('should include supplier from author', () => {
    const result = makeSbomResult()
    const parsed = JSON.parse(serializeSpdx(result))

    expect(parsed.packages[1].supplier).toBe('Person: John-David Dalton')
  })

  it('should omit supplier when author is absent', () => {
    const result = makeSbomResult()
    result.components[0].author = undefined
    const parsed = JSON.parse(serializeSpdx(result))

    expect(parsed.packages[1].supplier).toBeUndefined()
  })

  it('should include checksums from integrity', () => {
    const result = makeSbomResult()
    const parsed = JSON.parse(serializeSpdx(result))

    const lodashPkg = parsed.packages[1]
    expect(lodashPkg.checksums).toBeDefined()
    expect(lodashPkg.checksums.length).toBeGreaterThan(0)
    expect(lodashPkg.checksums[0].algorithm).toBeDefined()
    expect(lodashPkg.checksums[0].checksumValue).toBeDefined()
  })

  it('should deduplicate relationships', () => {
    const result = makeSbomResult()
    // Add a duplicate relationship
    result.relationships.push(
      { from: 'pkg:npm/my-app@1.0.0', to: 'pkg:npm/lodash@4.17.21' }
    )
    const parsed = JSON.parse(serializeSpdx(result))

    const dependsOn = parsed.relationships.filter(
      (r: { relationshipType: string }) => r.relationshipType === 'DEPENDS_ON'
    )
    expect(dependsOn).toHaveLength(1)
  })

  it('should use APPLICATION for application root type', () => {
    const result = makeSbomResult()
    result.rootComponent.type = 'application'
    const parsed = JSON.parse(serializeSpdx(result))

    expect(parsed.packages[0].primaryPackagePurpose).toBe('APPLICATION')
  })

  it('should use LIBRARY for library root type', () => {
    const result = makeSbomResult()
    const parsed = JSON.parse(serializeSpdx(result))

    expect(parsed.packages[0].primaryPackagePurpose).toBe('LIBRARY')
  })
})
