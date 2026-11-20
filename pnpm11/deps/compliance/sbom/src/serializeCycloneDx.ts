import crypto from 'node:crypto'

import { DepType } from '@pnpm/lockfile.detect-dep-types'

import { integrityToHashes } from './integrity.js'
import { classifyLicense } from './license.js'
import { encodePurlName } from './purl.js'
import type { SbomComponent, SbomRelationship, SbomResult, SbomRootComponent } from './types.js'

export interface CycloneDxOptions {
  pnpmVersion?: string
  lockfileOnly?: boolean
  sbomAuthors?: string[]
  sbomSupplier?: string
  specVersion?: string
  compact?: boolean
}

export function serializeCycloneDx (result: SbomResult, opts?: CycloneDxOptions): string {
  const { rootComponent, components, relationships } = result
  const rootBomRef = `pkg:npm/${encodePurlName(rootComponent.name)}@${rootComponent.version}`

  const bomComponents = components.map(createCycloneDxComponent)
  const bomDependencies = groupCycloneDxDependencies(rootBomRef, components, relationships)
  const metadata = createCycloneDxMetadata(rootComponent, rootBomRef, opts)
  const version = opts?.specVersion || '1.7'

  const bom: Record<string, unknown> = {
    $schema: `http://cyclonedx.org/schema/bom-${version}.schema.json`,
    bomFormat: 'CycloneDX',
    specVersion: version,
    serialNumber: `urn:uuid:${crypto.randomUUID()}`,
    version: 1,
    metadata,
    components: bomComponents,
    dependencies: bomDependencies,
  }

  return JSON.stringify(bom, null, opts?.compact ? undefined : 2)
}

function createCycloneDxComponent (comp: SbomComponent): Record<string, unknown> {
  const { group, name } = splitScopedName(comp.name)
  const cdxComp: Record<string, unknown> = {
    type: 'library',
    name,
    version: comp.version,
    purl: comp.purl,
    'bom-ref': comp.purl,
  }

  if (comp.depType === DepType.DevOnly) {
    cdxComp.scope = 'excluded'
    cdxComp.properties = [{ name: 'cdx:npm:package:development', value: 'true' }]
  }
  if (group) cdxComp.group = group
  if (comp.description) cdxComp.description = comp.description
  if (comp.author) cdxComp.authors = [{ name: comp.author }]
  if (comp.license) cdxComp.licenses = [classifyLicense(comp.license)]

  const externalRefs = buildComponentExternalRefs(comp)
  if (externalRefs.length > 0) {
    cdxComp.externalReferences = externalRefs
  }

  return cdxComp
}

function buildComponentExternalRefs (comp: SbomComponent): Array<Record<string, unknown>> {
  const externalRefs: Array<Record<string, unknown>> = []

  if (comp.tarballUrl) {
    const hashes = integrityToHashes(comp.integrity)
    const distRef: Record<string, unknown> = {
      type: 'distribution',
      url: comp.tarballUrl,
    }
    if (hashes.length > 0) {
      distRef.hashes = hashes.map((hash) => ({
        alg: hash.algorithm,
        content: hash.digest,
      }))
    }
    externalRefs.push(distRef)
  }
  if (comp.homepage) {
    externalRefs.push({ type: 'website', url: comp.homepage })
  }
  if (comp.repository) {
    externalRefs.push({ type: 'vcs', url: comp.repository })
  }
  if (comp.bugsUrl) {
    externalRefs.push({ type: 'issue-tracker', url: comp.bugsUrl })
  }

  return externalRefs
}

function groupCycloneDxDependencies (
  rootBomRef: string,
  components: SbomComponent[],
  relationships: SbomRelationship[]
): Array<{ ref: string, dependsOn: string[] }> {
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

  return Array.from(depMap.entries()).map(([ref, dependsOn]) => ({
    ref,
    dependsOn: [...new Set(dependsOn)],
  }))
}

function createCycloneDxRootComponent (
  rootComponent: SbomRootComponent,
  rootBomRef: string
): Record<string, unknown> {
  const { group: rootGroup, name: rootName } = splitScopedName(rootComponent.name)
  const rootCdxComponent: Record<string, unknown> = {
    type: rootComponent.type,
    name: rootName,
    version: rootComponent.version,
    purl: rootBomRef,
    'bom-ref': rootBomRef,
  }
  if (rootGroup) rootCdxComponent.group = rootGroup
  if (rootComponent.author) rootCdxComponent.authors = [{ name: rootComponent.author }]
  if (rootComponent.license) rootCdxComponent.licenses = [classifyLicense(rootComponent.license)]
  if (rootComponent.description) rootCdxComponent.description = rootComponent.description

  const rootExternalRefs = buildRootExternalRefs(rootComponent)
  if (rootExternalRefs.length > 0) {
    rootCdxComponent.externalReferences = rootExternalRefs
  }

  return rootCdxComponent
}

function buildRootExternalRefs (rootComponent: SbomRootComponent): Array<Record<string, unknown>> {
  const rootExternalRefs: Array<Record<string, unknown>> = []
  if (rootComponent.repository) {
    rootExternalRefs.push({ type: 'vcs', url: rootComponent.repository })
  }
  if (rootComponent.bugsUrl) {
    rootExternalRefs.push({ type: 'issue-tracker', url: rootComponent.bugsUrl })
  }
  return rootExternalRefs
}

function createCycloneDxMetadata (
  rootComponent: SbomRootComponent,
  rootBomRef: string,
  opts?: CycloneDxOptions
): Record<string, unknown> {
  const toolComponents: Array<Record<string, unknown>> = []
  if (opts?.pnpmVersion) {
    toolComponents.push({
      type: 'application',
      name: 'pnpm',
      version: opts.pnpmVersion,
    })
  }

  const metadata: Record<string, unknown> = {
    timestamp: new Date().toISOString(),
    lifecycles: [{ phase: opts?.lockfileOnly ? 'pre-build' : 'build' }],
    tools: { components: toolComponents },
    component: createCycloneDxRootComponent(rootComponent, rootBomRef),
  }
  if (opts?.sbomAuthors?.length) {
    metadata.authors = opts.sbomAuthors.map((name) => ({ name }))
  }
  if (opts?.sbomSupplier) {
    metadata.supplier = { name: opts.sbomSupplier }
  }

  return metadata
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
