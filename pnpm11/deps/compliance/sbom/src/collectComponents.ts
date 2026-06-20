import path from 'node:path'

import { DepType, type DepTypes, detectDepTypes } from '@pnpm/lockfile.detect-dep-types'
import type { LockfileObject, TarballResolution } from '@pnpm/lockfile.types'
import { nameVerFromPkgSnapshot, pkgSnapshotToResolution } from '@pnpm/lockfile.utils'
import {
  lockfileWalkerGroupImporterSteps,
  type LockfileWalkerStep,
} from '@pnpm/lockfile.walker'
import type { Resolution } from '@pnpm/resolving.resolver-base'
import { StoreIndex } from '@pnpm/store.index'
import type { DependenciesField, ProjectId, Registries } from '@pnpm/types'
import pLimit from 'p-limit'

import { getPkgMetadata, type GetPkgMetadataOptions } from './getPkgMetadata.js'
import { buildPurl, encodePurlName } from './purl.js'
import type { SbomComponent, SbomComponentType, SbomRelationship, SbomResult } from './types.js'

export interface WorkspacePackageInfo {
  name: string
  version: string
  license?: string
  description?: string
  author?: string
  repository?: string
}

export interface CollectSbomComponentsOptions {
  lockfile: LockfileObject
  rootName: string
  rootVersion: string
  rootLicense?: string
  rootDescription?: string
  rootAuthor?: string
  rootRepository?: string
  rootBugsUrl?: string
  sbomType?: SbomComponentType
  include?: { [dependenciesField in DependenciesField]: boolean }
  registries: Registries
  lockfileDir: string
  includedImporterIds?: ProjectId[]
  lockfileOnly?: boolean
  storeDir?: string
  virtualStoreDirMaxLength?: number
  workspacePackages?: Record<ProjectId, WorkspacePackageInfo>
  resolvedWorkspaceDeps?: ReturnType<typeof resolveWorkspaceDeps>
  // With auto-install-peers, peers resolve into the importer's `dependencies`
  // and are indistinguishable from real deps in the lockfile.
  excludePeerNamesByImporter?: Map<string, Set<string>>
}

const IMPORTER_WALK_CONCURRENCY = 8

export async function collectSbomComponents (opts: CollectSbomComponentsOptions): Promise<SbomResult> {
  const depTypes = detectDepTypes(opts.lockfile)
  const importerIds = opts.includedImporterIds ?? Object.keys(opts.lockfile.importers) as ProjectId[]

  const componentsMap = new Map<string, SbomComponent>()
  const relationships: SbomRelationship[] = []
  const rootPurl = `pkg:npm/${encodePurlName(opts.rootName)}@${opts.rootVersion}`

  const workspaceDeps = opts.resolvedWorkspaceDeps
    ?? (opts.lockfileOnly
      ? { links: [], additionalImporterIds: [] }
      : resolveWorkspaceDeps(opts.lockfile, importerIds, opts.include))
  const allImporterIds = [...importerIds, ...workspaceDeps.additionalImporterIds]

  // When excluding peers, walk each importer with its own `walked` set so one
  // importer's peer can't suppress another's real dependency.
  const importerWalkers = opts.excludePeerNamesByImporter
    ? allImporterIds.flatMap((importerId) =>
      lockfileWalkerGroupImporterSteps(opts.lockfile, [importerId], { include: opts.include }))
    : lockfileWalkerGroupImporterSteps(opts.lockfile, allImporterIds, { include: opts.include })

  const importerIdSet = new Set<string>(importerIds)

  if (opts.workspacePackages) {

    const workspaceDepTypes = new Map<string, DepType>()
    for (const dep of workspaceDeps.links) {
      const info = opts.workspacePackages[dep.targetImporterId]
      if (!info) continue
      const purl = buildPurl({ name: info.name, version: info.version })
      const current = workspaceDepTypes.get(purl)
      if (!dep.devOnly) {
        workspaceDepTypes.set(purl, DepType.ProdOnly)
      } else if (current === undefined) {
        workspaceDepTypes.set(purl, DepType.DevOnly)
      }
    }

    for (const dep of workspaceDeps.links) {
      const info = opts.workspacePackages[dep.targetImporterId]
      if (!info) continue

      const purl = buildPurl({ name: info.name, version: info.version })

      let parentPurl: string
      if (importerIdSet.has(dep.sourceImporterId)) {
        parentPurl = rootPurl
      } else {
        const sourceInfo = opts.workspacePackages[dep.sourceImporterId]
        parentPurl = sourceInfo
          ? buildPurl({ name: sourceInfo.name, version: sourceInfo.version })
          : rootPurl
      }
      relationships.push({ from: parentPurl, to: purl })

      if (!componentsMap.has(purl)) {
        componentsMap.set(purl, {
          name: info.name,
          version: info.version,
          purl,
          depPath: `link:${dep.targetImporterId}`,
          depType: workspaceDepTypes.get(purl) ?? DepType.ProdOnly,
          license: info.license,
          description: info.description,
          author: info.author,
          repository: info.repository,
        })
      }
    }
  }

  const storeIndex = (!opts.lockfileOnly && opts.storeDir)
    ? new StoreIndex(opts.storeDir)
    : undefined
  const metadataOpts: GetPkgMetadataOptions | undefined = (storeIndex && opts.storeDir)
    ? {
      storeDir: opts.storeDir,
      storeIndex,
      lockfileDir: opts.lockfileDir,
      virtualStoreDirMaxLength: opts.virtualStoreDirMaxLength ?? 120,
    }
    : undefined

  const walkImporter = pLimit(IMPORTER_WALK_CONCURRENCY)
  await Promise.all(
    importerWalkers.map(({ importerId, step }) => walkImporter(async () => {
      let parentPurl = rootPurl
      if (!importerIdSet.has(importerId as ProjectId)) {
        const info = opts.workspacePackages?.[importerId as ProjectId]
        // A reachable workspace importer with no resolved package info (e.g. its
        // manifest could not be read) is skipped entirely; walking it would
        // misattribute its dependencies to the root component.
        if (!info) return
        parentPurl = buildPurl({ name: info.name, version: info.version })
      }
      // Drop this importer's peer entries before walking. With the per-importer
      // walk above, this prunes a peer's exclusive subtree without hiding a
      // package that is also a real dependency here or in another importer.
      const peerNames = opts.excludePeerNamesByImporter?.get(importerId)
      const filteredStep = (peerNames?.size)
        ? {
          ...step,
          dependencies: step.dependencies.filter((dep) => {
            const { name } = nameVerFromPkgSnapshot(dep.depPath, dep.pkgSnapshot)
            return !name || !peerNames.has(name)
          }),
        }
        : step
      await walkStep(
        filteredStep,
        parentPurl,
        depTypes,
        componentsMap,
        relationships,
        opts,
        metadataOpts
      )
    }))
  )
  storeIndex?.close()

  return {
    rootComponent: {
      name: opts.rootName,
      version: opts.rootVersion,
      type: opts.sbomType ?? 'library',
      license: opts.rootLicense,
      description: opts.rootDescription,
      author: opts.rootAuthor,
      repository: opts.rootRepository,
      bugsUrl: opts.rootBugsUrl,
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
      const tarballUrl = (resolution as TarballResolution).tarball ?? gitDownloadUrl(resolution)

      let metadata: { license?: string, description?: string, author?: string, homepage?: string, repository?: string, bugsUrl?: string } = {}
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

export function gitDownloadUrl (resolution: Resolution): string | undefined {
  if (resolution.type !== 'git') return undefined
  const needsGitPlusPrefix = resolution.repo.includes('://') && !resolution.repo.startsWith('git+')
  const prefix = needsGitPlusPrefix ? 'git+' : ''
  return `${prefix}${resolution.repo}#${resolution.commit}`
}

interface WorkspaceLink {
  sourceImporterId: ProjectId
  targetImporterId: ProjectId
  depName: string
  devOnly: boolean
}

export function resolveWorkspaceDeps (
  lockfile: LockfileObject,
  importerIds: ProjectId[],
  include?: { [dependenciesField in DependenciesField]: boolean }
): { links: WorkspaceLink[], additionalImporterIds: ProjectId[] } {
  const links: WorkspaceLink[] = []
  const visited = new Set<string>(importerIds)
  const queue = [...importerIds]
  const additionalImporterIds: ProjectId[] = []

  for (let head = 0; head < queue.length; head++) {
    const importerId = queue[head]
    const snapshot = lockfile.importers[importerId]
    if (!snapshot) continue

    const devDepNames = new Set(Object.keys(snapshot.devDependencies ?? {}))
    const prodDeps = {
      ...(include?.dependencies !== false ? snapshot.dependencies : {}),
      ...(include?.optionalDependencies !== false ? snapshot.optionalDependencies : {}),
    }
    const allDeps: Record<string, string> = {
      ...prodDeps,
      ...(include?.devDependencies !== false ? snapshot.devDependencies : {}),
    }

    for (const [depName, reference] of Object.entries(allDeps)) {
      if (!reference.startsWith('link:')) continue

      const linkPath = reference.slice(5)
      const targetId = path.posix.normalize(
        importerId === ('.' as ProjectId) ? linkPath : path.posix.join(importerId, linkPath)
      ) as ProjectId

      // A crafted lockfile can point a `link:` target outside the workspace root;
      // such importer IDs must never be followed, as they later become filesystem reads.
      if (path.posix.isAbsolute(targetId) || targetId === '..' || targetId.startsWith('../')) continue

      // `in` would also match inherited keys (e.g. "toString"); a crafted lockfile
      // must not be able to enqueue importer IDs that are not actually present.
      if (!Object.prototype.hasOwnProperty.call(lockfile.importers, targetId)) continue

      const devOnly = devDepNames.has(depName) && !(depName in prodDeps)
      links.push({ sourceImporterId: importerId, targetImporterId: targetId, depName, devOnly })

      if (!visited.has(targetId)) {
        visited.add(targetId)
        additionalImporterIds.push(targetId)
        queue.push(targetId)
      }
    }
  }

  return { links, additionalImporterIds }
}
