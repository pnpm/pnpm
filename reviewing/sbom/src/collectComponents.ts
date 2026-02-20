import { type LockfileObject, type TarballResolution } from '@pnpm/lockfile.types'
import { nameVerFromPkgSnapshot, pkgSnapshotToResolution } from '@pnpm/lockfile.utils'
import {
  lockfileWalkerGroupImporterSteps,
  type LockfileWalkerStep,
} from '@pnpm/lockfile.walker'
import { type DepTypes, DepType, detectDepTypes } from '@pnpm/lockfile.detect-dep-types'
import { type DependenciesField, type ProjectId, type Registries } from '@pnpm/types'
import { buildPurl, encodePurlName } from './purl.js'
import { getPkgMetadata, type GetPkgMetadataOptions } from './getPkgMetadata.js'
import { type SbomComponent, type SbomRelationship, type SbomResult, type SbomComponentType } from './types.js'

export interface CollectSbomComponentsOptions {
  lockfile: LockfileObject
  rootName: string
  rootVersion: string
  rootLicense?: string
  rootDescription?: string
  rootAuthor?: string
  rootRepository?: string
  sbomType?: SbomComponentType
  include?: { [dependenciesField in DependenciesField]: boolean }
  registries: Registries
  lockfileDir: string
  includedImporterIds?: ProjectId[]
  lockfileOnly?: boolean
  storeDir?: string
  virtualStoreDirMaxLength?: number
}

export async function collectSbomComponents (opts: CollectSbomComponentsOptions): Promise<SbomResult> {
  const depTypes = detectDepTypes(opts.lockfile)
  const importerIds = opts.includedImporterIds ?? Object.keys(opts.lockfile.importers) as ProjectId[]

  const importerWalkers = lockfileWalkerGroupImporterSteps(
    opts.lockfile,
    importerIds,
    { include: opts.include }
  )

  const componentsMap = new Map<string, SbomComponent>()
  const relationships: SbomRelationship[] = []
  const rootPurl = `pkg:npm/${encodePurlName(opts.rootName)}@${opts.rootVersion}`

  const metadataOpts: GetPkgMetadataOptions | undefined = (!opts.lockfileOnly && opts.storeDir)
    ? {
      storeDir: opts.storeDir,
      lockfileDir: opts.lockfileDir,
      virtualStoreDirMaxLength: opts.virtualStoreDirMaxLength ?? 120,
    }
    : undefined

  await Promise.all(
    importerWalkers.map(async ({ step }) => {
      await walkStep(
        step,
        rootPurl,
        depTypes,
        componentsMap,
        relationships,
        opts,
        metadataOpts
      )
    })
  )

  return {
    rootComponent: {
      name: opts.rootName,
      version: opts.rootVersion,
      type: opts.sbomType ?? 'library',
      license: opts.rootLicense,
      description: opts.rootDescription,
      author: opts.rootAuthor,
      repository: opts.rootRepository,
    },
    components: Array.from(componentsMap.values()),
    relationships,
  }
}

async function walkStep (
  step: LockfileWalkerStep,
  parentPurl: string,
  depTypes: DepTypes,
  componentsMap: Map<string, SbomComponent>,
  relationships: SbomRelationship[],
  opts: CollectSbomComponentsOptions,
  metadataOpts: GetPkgMetadataOptions | undefined
): Promise<void> {
  await Promise.all(
    step.dependencies.map(async (dep) => {
      const { depPath, pkgSnapshot, next } = dep
      const { name, version, nonSemverVersion } = nameVerFromPkgSnapshot(depPath, pkgSnapshot)

      if (!name || !version) return

      const purl = buildPurl({ name, version, nonSemverVersion: nonSemverVersion ?? undefined })

      relationships.push({ from: parentPurl, to: purl })

      if (componentsMap.has(purl)) return

      const integrity = (pkgSnapshot.resolution as TarballResolution).integrity
      const resolution = pkgSnapshotToResolution(depPath, pkgSnapshot, opts.registries)
      const tarballUrl = (resolution as TarballResolution).tarball

      let metadata: { license?: string, description?: string, author?: string, homepage?: string, repository?: string } = {}
      if (metadataOpts) {
        metadata = await getPkgMetadata(depPath, pkgSnapshot, opts.registries, metadataOpts)
      }

      const component: SbomComponent = {
        name,
        version,
        purl,
        depPath,
        depType: depTypes[depPath] ?? DepType.ProdOnly,
        integrity,
        tarballUrl,
        ...metadata,
      }

      componentsMap.set(purl, component)

      const subStep = next()
      await walkStep(subStep, purl, depTypes, componentsMap, relationships, opts, metadataOpts)
    })
  )
}

