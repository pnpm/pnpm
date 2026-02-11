import crypto from 'crypto'
import { integrityToHashes } from './integrity.js'
import { classifyLicense } from './license.js'
import { encodePurlName } from './purl.js'
import { type SbomResult } from './types.js'

export interface CycloneDxToolInfo {
  pnpmVersion: string
}

export function serializeCycloneDx (result: SbomResult, toolInfo?: CycloneDxToolInfo): string {
  const { rootComponent, components, relationships } = result

  const rootBomRef = `pkg:npm/${encodePurlName(rootComponent.name)}@${rootComponent.version}`

  const bomComponents = components.map((comp) => {
    const { group, name } = splitScopedName(comp.name)

    const cdxComp: Record<string, unknown> = {
      type: 'library',
      name,
      version: comp.version,
      purl: comp.purl,
      'bom-ref': comp.purl,
    }

    if (group) {
      cdxComp.group = group
    }

    if (comp.description) {
      cdxComp.description = comp.description
    }

    if (comp.author) {
      cdxComp.authors = [{ name: comp.author }]
    }

    if (comp.license) {
      cdxComp.licenses = [classifyLicense(comp.license)]
    }

    const externalRefs: Array<Record<string, unknown>> = []

    const hashes = integrityToHashes(comp.integrity)
    if (hashes.length > 0) {
      externalRefs.push({
        type: 'distribution',
        url: comp.tarballUrl ?? comp.purl,
        hashes: hashes.map((h) => ({
          alg: h.algorithm,
          content: h.digest,
        })),
      })
    }

    if (comp.homepage) {
      externalRefs.push({
        type: 'website',
        url: comp.homepage,
      })
    }

    if (externalRefs.length > 0) {
      cdxComp.externalReferences = externalRefs
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

  const { group: rootGroup, name: rootName } = splitScopedName(rootComponent.name)

  const rootCdxComponent: Record<string, unknown> = {
    type: rootComponent.type,
    name: rootName,
    version: rootComponent.version,
    'bom-ref': rootBomRef,
  }
  if (rootGroup) {
    rootCdxComponent.group = rootGroup
  }

  const toolComponents: Array<Record<string, unknown>> = []
  if (toolInfo) {
    toolComponents.push({
      type: 'application',
      name: 'pnpm',
      version: toolInfo.pnpmVersion,
    })
  }

  const bom: Record<string, unknown> = {
    bomFormat: 'CycloneDX',
    specVersion: '1.7',
    serialNumber: `urn:uuid:${crypto.randomUUID()}`,
    version: 1,
    metadata: {
      tools: { components: toolComponents },
      component: rootCdxComponent,
    },
    components: bomComponents,
    dependencies: bomDependencies,
  }

  return JSON.stringify(bom, null, 2)
}

function splitScopedName (fullName: string): { group: string | undefined, name: string } {
  if (fullName.startsWith('@')) {
    const slashIdx = fullName.indexOf('/')
    if (slashIdx > 0) {
      return { group: fullName.slice(0, slashIdx), name: fullName.slice(slashIdx + 1) }
    }
  }
  return { group: undefined, name: fullName }
}
