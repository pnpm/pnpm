import { normalizeRegistriesByPrefix } from '@pnpm/config.normalize-registries'
import { checkPackageInstallability } from '@pnpm/config.package-is-installable'
import { PnpmError } from '@pnpm/error'
import { DepType, type DepTypes, detectDepTypes } from '@pnpm/lockfile.detect-dep-types'
import type { LockfileObject, LockfileResolution, TarballResolution } from '@pnpm/lockfile.types'
import { nameVerFromPkgSnapshot, pkgSnapshotToResolution } from '@pnpm/lockfile.utils'
import {
  lockfileWalkerGroupImporterSteps,
  type LockfileWalkerStep,
} from '@pnpm/lockfile.walker'
import type { Resolution } from '@pnpm/resolving.resolver-base'
import { StoreIndex } from '@pnpm/store.index'
import type { DependenciesField, ProjectId, RegistriesByScope, SupportedArchitectures } from '@pnpm/types'
import pLimit from 'p-limit'

import { getPkgMetadata, type GetPkgMetadataOptions } from './getPkgMetadata.js'
import { buildPurl, encodePurlName } from './purl.js'
import type { SbomComponent, SbomComponentType, SbomRelationship, SbomResult } from './types.js'
import {
  addWorkspaceComponents,
  resolveWorkspaceDeps,
  type WorkspaceComponentsTarget,
  type WorkspacePackageInfo,
} from './workspaceDeps.js'

export { resolveWorkspaceDeps, type WorkspacePackageInfo } from './workspaceDeps.js'

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
  registriesByScope: RegistriesByScope
  registriesByPrefix?: Record<string, string>
  lockfileDir: string
  includedImporterIds?: ProjectId[]
  resolvePeersFromWorkspaceRoot?: boolean
  supportedArchitectures?: SupportedArchitectures
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
  const depTypes = detectDepTypes(opts.lockfile, opts)
  const importerIds = opts.includedImporterIds ?? Object.keys(opts.lockfile.importers) as ProjectId[]
  const rootPurl = `pkg:npm/${encodePurlName(opts.rootName)}@${opts.rootVersion}`

  const workspaceDeps = opts.resolvedWorkspaceDeps
    ?? (opts.lockfileOnly
      ? { links: [], additionalImporterIds: [] }
      : resolveWorkspaceDeps(opts.lockfile, importerIds, opts.include))
  const importerWalkers = createImporterWalkers(opts, [...importerIds, ...workspaceDeps.additionalImporterIds])
  const importerIdSet = new Set<string>(importerIds)

  const components: WorkspaceComponentsTarget = { componentsMap: new Map(), relationships: [] }
  if (opts.workspacePackages) {
    addWorkspaceComponents(components, {
      workspacePackages: opts.workspacePackages,
      links: workspaceDeps.links,
      importerIdSet,
      rootPurl,
    })
  }

  await walkImporters({ ...components, depTypes, opts, metadataOpts: undefined }, { importerWalkers, importerIdSet, rootPurl })

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
    components: Array.from(components.componentsMap.values()),
    relationships: components.relationships,
  }
}

type ImporterWalker = ReturnType<typeof lockfileWalkerGroupImporterSteps>[number]
type WalkedDependency = LockfileWalkerStep['dependencies'][number]

interface WalkContext {
  componentsMap: Map<string, SbomComponent>
  relationships: SbomRelationship[]
  depTypes: DepTypes
  opts: CollectSbomComponentsOptions
  metadataOpts: GetPkgMetadataOptions | undefined
}

interface ImporterParents {
  importerIdSet: Set<string>
  rootPurl: string
}

function createImporterWalkers (opts: CollectSbomComponentsOptions, allImporterIds: ProjectId[]): ImporterWalker[] {
  // When excluding peers, walk each importer with its own `walked` set so one
  // importer's peer can't suppress another's real dependency.
  const walkerOpts = { include: opts.include, resolvePeersFromWorkspaceRoot: opts.resolvePeersFromWorkspaceRoot }
  return opts.excludePeerNamesByImporter
    ? allImporterIds.flatMap((importerId) =>
      lockfileWalkerGroupImporterSteps(opts.lockfile, [importerId], walkerOpts))
    : lockfileWalkerGroupImporterSteps(opts.lockfile, allImporterIds, walkerOpts)
}

async function walkImporters (
  ctx: WalkContext,
  { importerWalkers, ...parents }: ImporterParents & { importerWalkers: ImporterWalker[] }
): Promise<void> {
  const { opts } = ctx
  const storeIndex = (!opts.lockfileOnly && opts.storeDir)
    ? new StoreIndex(opts.storeDir)
    : undefined
  const walkCtx: WalkContext = {
    ...ctx,
    metadataOpts: (storeIndex && opts.storeDir)
      ? {
        storeDir: opts.storeDir,
        storeIndex,
        lockfileDir: opts.lockfileDir,
        virtualStoreDirMaxLength: opts.virtualStoreDirMaxLength ?? 120,
      }
      : undefined,
  }

  const walkImporter = pLimit(IMPORTER_WALK_CONCURRENCY)
  await Promise.all(
    importerWalkers.map((importerWalker) => walkImporter(async () => walkImporterDependencies(walkCtx, importerWalker, parents)))
  )
  storeIndex?.close()
}

async function walkImporterDependencies (
  ctx: WalkContext,
  { importerId, step }: ImporterWalker,
  parents: ImporterParents
): Promise<void> {
  const parentPurl = getImporterPurl(ctx.opts, importerId as ProjectId, parents)
  if (parentPurl == null) return
  await walkStep(ctx, omitPeerDependencies(ctx.opts, importerId, step), parentPurl)
}

function getImporterPurl (
  opts: CollectSbomComponentsOptions,
  importerId: ProjectId,
  { importerIdSet, rootPurl }: ImporterParents
): string | undefined {
  if (importerIdSet.has(importerId)) return rootPurl
  const info = opts.workspacePackages?.[importerId]
  // A reachable workspace importer with no resolved package info (e.g. its
  // manifest could not be read) is skipped entirely; walking it would
  // misattribute its dependencies to the root component.
  if (!info) return undefined
  return buildPurl({ name: info.name, version: info.version })
}

/**
 * Drops this importer's peer entries before walking. With the per-importer
 * walk, this prunes a peer's exclusive subtree without hiding a package that
 * is also a real dependency here or in another importer.
 */
function omitPeerDependencies (
  opts: CollectSbomComponentsOptions,
  importerId: string,
  step: LockfileWalkerStep
): LockfileWalkerStep {
  const peerNames = opts.excludePeerNamesByImporter?.get(importerId)
  if (!peerNames?.size) return step
  return {
    ...step,
    dependencies: step.dependencies.filter((dep) => {
      const { name } = nameVerFromPkgSnapshot(dep.depPath, dep.pkgSnapshot)
      return !name || !peerNames.has(name)
    }),
  }
}

async function walkStep (
  ctx: WalkContext,
  step: LockfileWalkerStep,
  parentPurl: string
): Promise<void> {
  await Promise.all(
    step.dependencies.map(async (dep) => walkDependency(ctx, dep, parentPurl))
  )
}

async function walkDependency (
  ctx: WalkContext,
  dep: WalkedDependency,
  parentPurl: string
): Promise<void> {
  const { depPath, pkgSnapshot } = dep
  const { name, version, nonSemverVersion, registryName } = nameVerFromPkgSnapshot(depPath, pkgSnapshot)

  if (!name || !version) return
  if (isUninstallableOptionalDependency(ctx.opts, dep, { name, version })) return

  const purl = buildDependencyPurl(ctx.opts, { depPath, name, version, nonSemverVersion, registryName })

  ctx.relationships.push({ from: parentPurl, to: purl })

  if (ctx.componentsMap.has(purl)) return

  const component = await describeComponent(ctx, dep, { name, version, purl })
  ctx.componentsMap.set(purl, component)

  await walkStep(ctx, dep.next(), purl)
}

/**
 * An optional dependency for another platform is in the lockfile but was
 * never fetched, so the store holds no metadata to describe it with.
 * `checkPackageInstallability`, not `packageIsInstallable`: the latter
 * reports every skip through the install loggers, which reading a
 * lockfile must not do.
 */
function isUninstallableOptionalDependency (
  opts: CollectSbomComponentsOptions,
  { depPath, pkgSnapshot }: WalkedDependency,
  { name, version }: { name: string, version: string }
): boolean {
  return !opts.lockfileOnly && pkgSnapshot.optional === true && checkPackageInstallability(pkgSnapshot.id ?? depPath, {
    name,
    version,
    cpu: pkgSnapshot.cpu,
    os: pkgSnapshot.os,
    libc: pkgSnapshot.libc,
  }, {
    optional: true,
    supportedArchitectures: opts.supportedArchitectures,
  }) != null
}

function buildDependencyPurl (
  opts: CollectSbomComponentsOptions,
  { depPath, name, version, nonSemverVersion, registryName }: {
    depPath: string
    name: string
    version: string
    nonSemverVersion?: string | null
    registryName?: string | null
  }
): string {
  // Resolve the alias before the purl is built. An unknown alias would
  // otherwise yield an unqualified purl that collides with the same
  // package from the default registry, and the `componentsMap.has(purl)`
  // shortcut in `walkDependency` would drop this component before
  // `pkgSnapshotToResolution` ever got to reject it — silently omitting an
  // artifact from a compliance document.
  const registryUrl = registryName == null
    ? undefined
    : normalizeRegistriesByPrefix(opts.registriesByPrefix)[registryName]
  if (registryName != null && registryUrl == null) {
    throw new PnpmError('MISSING_NAMED_REGISTRY',
      `Cannot describe package "${depPath}": it was resolved from the named registry '${registryName}:', which is not present in the registriesByPrefix setting.`,
      { hint: `Add '${registryName}' to the registriesByPrefix setting in pnpm-workspace.yaml.` })
  }

  return buildPurl({
    name,
    version,
    nonSemverVersion: nonSemverVersion ?? undefined,
    registryUrl,
  })
}

async function describeComponent (
  ctx: WalkContext,
  { depPath, pkgSnapshot }: WalkedDependency,
  { name, version, purl }: { name: string, version: string, purl: string }
): Promise<SbomComponent> {
  const { opts } = ctx
  const integrity = verifiedIntegrity(pkgSnapshot.resolution)
  const resolution = pkgSnapshotToResolution(depPath, pkgSnapshot, { registriesByScope: opts.registriesByScope, registriesByPrefix: opts.registriesByPrefix })
  const tarballUrl = (resolution as TarballResolution).tarball ?? gitDownloadUrl(resolution)

  let metadata: { license?: string, description?: string, author?: string, homepage?: string, repository?: string, bugsUrl?: string } = {}
  if (ctx.metadataOpts) {
    metadata = await getPkgMetadata(depPath, pkgSnapshot, { registriesByScope: opts.registriesByScope, registriesByPrefix: opts.registriesByPrefix }, ctx.metadataOpts)
  }

  return {
    name,
    version,
    purl,
    depPath,
    depType: ctx.depTypes[depPath] ?? DepType.ProdOnly,
    integrity,
    tarballUrl,
    ...metadata,
  }
}

export function gitDownloadUrl (resolution: Resolution): string | undefined {
  if (resolution.type !== 'git') return undefined
  const needsGitPlusPrefix = resolution.repo.includes('://') && !resolution.repo.startsWith('git+')
  const prefix = needsGitPlusPrefix ? 'git+' : ''
  return `${prefix}${resolution.repo}#${resolution.commit}`
}

/**
 * The resolution's integrity, but only where pnpm verifies the downloaded
 * bytes against it: the tarball/registry hash and a `type: binary` runtime
 * archive's. Nothing checks a git checkout against a hash, so an `integrity`
 * recorded on one is not a checksum and is never published as one.
 *
 * Read from an untyped lockfile, so the shape is probed rather than trusted:
 * reading a checksum out of a malformed resolution yields nothing instead of
 * throwing, and a non-string integrity never reaches `ssri.parse` downstream.
 * (A malformed resolution still fails the walk further along, in
 * `pkgSnapshotToResolution` — this only keeps the checksum lookup total.)
 */
function verifiedIntegrity (resolution: LockfileResolution): string | undefined {
  const { type, integrity } = (resolution ?? {}) as { type?: string, integrity?: unknown }
  if (typeof integrity !== 'string') return undefined
  return (type === undefined || type === 'binary') ? integrity : undefined
}
