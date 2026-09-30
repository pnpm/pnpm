import crypto from 'node:crypto'

import { integrityToHashes } from './integrity.js'
import { encodePurlName } from './purl.js'
import type { SbomComponent, SbomRelationship, SbomResult, SbomRootComponent } from './types.js'

export interface SpdxOptions {
  compact?: boolean
}

export function serializeSpdx (result: SbomResult, opts?: SpdxOptions): string {
  const { rootComponent, components, relationships } = result
  const rootSpdxId = 'SPDXRef-RootPackage'
  const rootPurl = `pkg:npm/${encodePurlName(rootComponent.name)}@${rootComponent.version}`

  const purlToSpdxId = new Map<string, string>()
  purlToSpdxId.set(rootPurl, rootSpdxId)

  const rootPackage = createSpdxRootPackage(rootComponent, rootSpdxId, rootPurl)
  const spdxPackages = components.map((comp, idx) => createSpdxComponentPackage(comp, idx, purlToSpdxId))
  const spdxRelationships = buildSpdxRelationships(rootSpdxId, relationships, purlToSpdxId)

  const doc = {
    spdxVersion: 'SPDX-2.3',
    dataLicense: 'CC0-1.0',
    SPDXID: 'SPDXRef-DOCUMENT',
    name: rootComponent.name,
    documentNamespace: `https://spdx.org/spdxdocs/${sanitizeSpdxId(rootComponent.name)}-${rootComponent.version}-${crypto.randomUUID()}`,
    creationInfo: {
      created: `${new Date().toISOString().split('.')[0]}Z`,
      creators: [
        'Tool: pnpm',
      ],
    },
    packages: [rootPackage, ...spdxPackages],
    relationships: spdxRelationships,
  }

  return JSON.stringify(doc, null, opts?.compact ? undefined : 2)
}

function createSpdxRootPackage (
  rootComponent: SbomRootComponent,
  rootSpdxId: string,
  rootPurl: string
): Record<string, unknown> {
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
    licenseConcluded: rootComponent.license ?? 'NOASSERTION',
    licenseDeclared: rootComponent.license ?? 'NOASSERTION',
    copyrightText: 'NOASSERTION',
  }

  if (rootComponent.description) rootPackage.description = rootComponent.description
  if (rootComponent.author) rootPackage.supplier = `Person: ${rootComponent.author}`
  if (rootComponent.repository) rootPackage.homepage = rootComponent.repository

  return rootPackage
}

function createSpdxComponentPackage (
  comp: SbomComponent,
  idx: number,
  purlToSpdxId: Map<string, string>
): Record<string, unknown> {
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
    licenseConcluded: comp.license ?? 'NOASSERTION',
    licenseDeclared: comp.license ?? 'NOASSERTION',
    copyrightText: 'NOASSERTION',
  }

  if (comp.description) pkg.description = comp.description
  if (comp.homepage) pkg.homepage = comp.homepage
  if (comp.author) pkg.supplier = `Person: ${comp.author}`

  const hashes = integrityToHashes(comp.integrity)
  if (hashes.length > 0) {
    pkg.checksums = hashes.map((hash) => ({
      algorithm: spdxHashAlgorithm(hash.algorithm),
      checksumValue: hash.digest,
    }))
  }

  return pkg
}

function buildSpdxRelationships (
  rootSpdxId: string,
  relationships: SbomRelationship[],
  purlToSpdxId: Map<string, string>
): Array<Record<string, unknown>> {
  const spdxRelationships: Array<Record<string, unknown>> = [
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

  return spdxRelationships
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
