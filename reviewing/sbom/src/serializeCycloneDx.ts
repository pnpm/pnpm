import crypto from 'crypto'
import { integrityToHashes } from './integrity.js'
import { encodePurlName } from './purl.js'
import { type SbomResult } from './types.js'

export function serializeCycloneDx (result: SbomResult): string {
  const { rootComponent, components, relationships } = result

  const rootBomRef = `pkg:npm/${encodePurlName(rootComponent.name)}@${rootComponent.version}`

  const bomComponents = components.map((comp) => {
    const cdxComp: Record<string, unknown> = {
      type: 'library',
      name: comp.name,
      version: comp.version,
      purl: comp.purl,
      'bom-ref': comp.purl,
    }

    if (comp.description) {
      cdxComp.description = comp.description
    }

    if (comp.author) {
      cdxComp.authors = [{ name: comp.author }]
    }

    const hashes = integrityToHashes(comp.integrity)
    if (hashes.length > 0) {
      cdxComp.hashes = hashes.map((h) => ({
        alg: h.algorithm,
        content: h.digest,
      }))
    }

    if (comp.license) {
      cdxComp.licenses = [
        {
          license: {
            name: comp.license,
          },
        },
      ]
    }

    if (comp.homepage) {
      cdxComp.externalReferences = [
        {
          type: 'website',
          url: comp.homepage,
        },
      ]
    }

    return cdxComp
  })

  // Group relationships by source
  const depMap = new Map<string, string[]>()
  depMap.set(rootBomRef, [])
  for (const comp of components) {
    depMap.set(comp.purl, [])
  }
  for (const rel of relationships) {
    const deps = depMap.get(rel.from)
    if (deps) {
      deps.push(rel.to)
    }
  }

  const bomDependencies = Array.from(depMap.entries()).map(([ref, dependsOn]) => ({
    ref,
    dependsOn: [...new Set(dependsOn)],
  }))

  const bom = {
    bomFormat: 'CycloneDX',
    specVersion: '1.6',
    serialNumber: `urn:uuid:${crypto.randomUUID()}`,
    version: 1,
    metadata: {
      tools: [
        {
          name: 'pnpm',
        },
      ],
      component: {
        type: rootComponent.type,
        name: rootComponent.name,
        version: rootComponent.version,
        'bom-ref': rootBomRef,
      },
    },
    components: bomComponents,
    dependencies: bomDependencies,
  }

  return JSON.stringify(bom, null, 2)
}

