import crypto from 'crypto'
import { integrityToHashes } from './integrity.js'
import { encodePurlName } from './purl.js'
import { type SbomResult } from './types.js'

export function serializeSpdx (result: SbomResult): string {
  const { rootComponent, components, relationships } = result

  const rootSpdxId = 'SPDXRef-RootPackage'
  const documentNamespace = `https://spdx.org/spdxdocs/${sanitizeSpdxId(rootComponent.name)}-${rootComponent.version}-${crypto.randomUUID()}`

  const rootPurl = `pkg:npm/${encodePurlName(rootComponent.name)}@${rootComponent.version}`

  const rootPackage: Record<string, unknown> = {
    SPDXID: rootSpdxId,
    name: rootComponent.name,
    versionInfo: rootComponent.version,
    downloadLocation: 'NOASSERTION',
    filesAnalyzed: false,
    primaryPackagePurpose: rootComponent.type === 'application' ? 'APPLICATION' : 'LIBRARY',
    externalRefs: [
      {
        referenceCategory: 'PACKAGE-MANAGER',
        referenceType: 'purl',
        referenceLocator: rootPurl,
      },
    ],
  }

  if (rootComponent.license) {
    rootPackage.licenseConcluded = rootComponent.license
    rootPackage.licenseDeclared = rootComponent.license
  } else {
    rootPackage.licenseConcluded = 'NOASSERTION'
    rootPackage.licenseDeclared = 'NOASSERTION'
  }

  rootPackage.copyrightText = 'NOASSERTION'

  if (rootComponent.description) {
    rootPackage.description = rootComponent.description
  }

  if (rootComponent.author) {
    rootPackage.supplier = `Person: ${rootComponent.author}`
  }

  if (rootComponent.repository) {
    rootPackage.homepage = rootComponent.repository
  }

  const purlToSpdxId = new Map<string, string>()
  purlToSpdxId.set(rootPurl, rootSpdxId)

  const spdxPackages = components.map((comp, idx) => {
    const spdxId = `SPDXRef-Package-${sanitizeSpdxId(comp.name)}-${sanitizeSpdxId(comp.version)}-${idx}`
    purlToSpdxId.set(comp.purl, spdxId)

    const pkg: Record<string, unknown> = {
      SPDXID: spdxId,
      name: comp.name,
      versionInfo: comp.version,
      downloadLocation: comp.tarballUrl ?? 'NOASSERTION',
      filesAnalyzed: false,
      externalRefs: [
        {
          referenceCategory: 'PACKAGE-MANAGER',
          referenceType: 'purl',
          referenceLocator: comp.purl,
        },
      ],
    }

    if (comp.license) {
      pkg.licenseConcluded = comp.license
      pkg.licenseDeclared = comp.license
    } else {
      pkg.licenseConcluded = 'NOASSERTION'
      pkg.licenseDeclared = 'NOASSERTION'
    }

    pkg.copyrightText = 'NOASSERTION'

    if (comp.description) {
      pkg.description = comp.description
    }

    if (comp.homepage) {
      pkg.homepage = comp.homepage
    }

    if (comp.author) {
      pkg.supplier = `Person: ${comp.author}`
    }

    const hashes = integrityToHashes(comp.integrity)
    if (hashes.length > 0) {
      pkg.checksums = hashes.map((h) => ({
        algorithm: spdxHashAlgorithm(h.algorithm),
        checksumValue: h.digest,
      }))
    }

    return pkg
  })

  const spdxRelationships = [
    {
      spdxElementId: 'SPDXRef-DOCUMENT',
      relatedSpdxElement: rootSpdxId,
      relationshipType: 'DESCRIBES',
    },
  ]

  const seenRelationships = new Set<string>()
  for (const rel of relationships) {
    const fromId = purlToSpdxId.get(rel.from)
    const toId = purlToSpdxId.get(rel.to)
    if (fromId && toId) {
      const key = `${fromId}|${toId}`
      if (seenRelationships.has(key)) continue
      seenRelationships.add(key)
      spdxRelationships.push({
        spdxElementId: fromId,
        relatedSpdxElement: toId,
        relationshipType: 'DEPENDS_ON',
      })
    }
  }

  const doc = {
    spdxVersion: 'SPDX-2.3',
    dataLicense: 'CC0-1.0',
    SPDXID: 'SPDXRef-DOCUMENT',
    name: rootComponent.name,
    documentNamespace,
    creationInfo: {
      created: new Date().toISOString(),
      creators: [
        'Tool: pnpm',
      ],
    },
    packages: [rootPackage, ...spdxPackages],
    relationships: spdxRelationships,
  }

  return JSON.stringify(doc, null, 2)
}

function sanitizeSpdxId (value: string): string {
  return value.replace(/[^a-z0-9.-]/gi, '-')
}

function spdxHashAlgorithm (algo: string): string {
  switch (algo) {
  case 'SHA-1':
    return 'SHA1'
  case 'SHA-256':
    return 'SHA256'
  case 'SHA-384':
    return 'SHA384'
  case 'SHA-512':
    return 'SHA512'
  default:
    return algo
  }
}
