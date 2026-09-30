import path from 'node:path'

import { packageManager } from '@pnpm/cli.meta'
import { readProjectManifestOnly } from '@pnpm/cli.utils'
import { WANTED_LOCKFILE } from '@pnpm/constants'
import {
  authorNameFromField,
  bugsUrlFromField,
  collectSbomComponents,
  repositoryFromField,
  resolveWorkspaceDeps,
  type SbomComponentType,
  type SbomFormat,
  serializeCycloneDx,
  serializeSpdx,
  type WorkspacePackageInfo,
} from '@pnpm/deps.compliance.sbom'
import { PnpmError } from '@pnpm/error'
import { getLockfileImporterId } from '@pnpm/lockfile.fs'
import type { LockfileObject } from '@pnpm/lockfile.types'
import type { ProjectId, ProjectManifest } from '@pnpm/types'
import pLimit from 'p-limit'

import type { SbomCommandOptions } from './sbom.js'
import { type ManifestLike, resolveRootLicense, type SharedContext } from './sharedContext.js'

export interface SerializeOptions {
  format: SbomFormat
  sbomType: SbomComponentType
  sbomSpecVersion: string | undefined
}

type SbomResult = Awaited<ReturnType<typeof collectSbomComponents>>

interface ReachableWorkspacePackages {
  resolvedWorkspaceDeps: ReturnType<typeof resolveWorkspaceDeps> | undefined
  workspacePackages: Record<ProjectId, WorkspacePackageInfo> | undefined
}

interface RootComponentFields {
  rootName: string
  rootVersion: string
  rootLicense: string | undefined
  rootDescription: string | undefined
  rootAuthor: ReturnType<typeof authorNameFromField>
  rootRepository: ReturnType<typeof repositoryFromField>
  rootBugsUrl: ReturnType<typeof bugsUrlFromField>
}

export async function generateSbomForProject (
  opts: SbomCommandOptions,
  serialOpts: SerializeOptions,
  ctx: SharedContext,
  compact?: boolean
): Promise<{ output: string, exitCode: number, rootName: string, rootVersion: string }> {
  const { lockfile } = ctx

  const include = {
    dependencies: opts.production !== false,
    devDependencies: opts.dev !== false,
    optionalDependencies: opts.optional !== false,
  }

  const rootComponent = await describeRootComponent(opts, ctx)

  const lockfileDir = opts.lockfileDir ?? opts.dir
  const includedImporterIds = opts.selectedProjectsGraph
    ? Object.keys(opts.selectedProjectsGraph)
      .map((projectDir) => getLockfileImporterId(lockfileDir, projectDir))
    : undefined
  assertImportersAreInLockfile(lockfile, includedImporterIds)

  const { resolvedWorkspaceDeps, workspacePackages } = await resolveReachableWorkspacePackages({
    opts,
    ctx,
    include,
    includedImporterIds,
  })

  const result = await collectSbomComponents({
    lockfile,
    ...rootComponent,
    sbomType: serialOpts.sbomType,
    include,
    registriesByScope: opts.registriesByScope,
    registriesByPrefix: opts.registriesByPrefix,
    lockfileDir,
    includedImporterIds,
    resolvePeersFromWorkspaceRoot: opts.resolvePeersFromWorkspaceRoot,
    supportedArchitectures: opts.supportedArchitectures,
    lockfileOnly: opts.lockfileOnly,
    storeDir: ctx.storeDir,
    virtualStoreDirMaxLength: opts.virtualStoreDirMaxLength,
    workspacePackages,
    resolvedWorkspaceDeps,
    excludePeerNamesByImporter: ctx.excludePeerNamesByImporter,
  })

  const output = serializeSbom(result, { opts, serialOpts, compact })

  return { output, exitCode: 0, rootName: rootComponent.rootName, rootVersion: rootComponent.rootVersion }
}

async function resolveReachableWorkspacePackages (
  { opts, ctx, include, includedImporterIds }: {
    opts: SbomCommandOptions
    ctx: SharedContext
    include: Parameters<typeof resolveWorkspaceDeps>[2]
    includedImporterIds: ProjectId[] | undefined
  }
): Promise<ReachableWorkspacePackages> {
  if (opts.lockfileOnly) return { resolvedWorkspaceDeps: undefined, workspacePackages: undefined }
  const { lockfile } = ctx
  const resolvedWorkspaceDeps = resolveWorkspaceDeps(
    lockfile,
    includedImporterIds ?? Object.keys(lockfile.importers) as ProjectId[],
    include
  )
  const workspacePackages = await buildWorkspacePackagesMap(
    resolvedWorkspaceDeps.additionalImporterIds,
    opts.lockfileDir ?? opts.dir,
    ctx.workspaceManifestsByImporterId
  )
  return { resolvedWorkspaceDeps, workspacePackages }
}

async function describeRootComponent (opts: SbomCommandOptions, ctx: SharedContext): Promise<RootComponentFields> {
  const { rootManifest, rootLicense: cachedRootLicense } = ctx

  const selectedEntries = opts.selectedProjectsGraph
    ? Object.entries(opts.selectedProjectsGraph)
    : undefined
  const singleProject = selectedEntries?.length === 1
    ? selectedEntries[0]
    : undefined

  const manifest = singleProject
    ? singleProject[1].package.manifest
    : rootManifest

  const rootName = manifest.name ?? 'unknown'
  const rootVersion = manifest.version ?? '0.0.0'
  const rootLicense = singleProject
    ? await resolveSelectedProjectLicense(manifest, singleProject[0], cachedRootLicense)
    : cachedRootLicense
  return {
    rootName,
    rootVersion,
    rootLicense,
    rootDescription: rootComponentField(manifest, rootManifest, 'description'),
    rootAuthor: authorNameFromField(rootComponentField(manifest, rootManifest, 'author')),
    rootRepository: repositoryFromField(rootComponentField(manifest, rootManifest, 'repository')),
    rootBugsUrl: bugsUrlFromField(rootComponentField(manifest, rootManifest, 'bugs')),
  }
}

async function resolveSelectedProjectLicense (
  manifest: ProjectManifest,
  projectDir: string,
  workspaceRootLicense: string | undefined
): Promise<string | undefined> {
  const declaresLicense = 'license' in manifest || 'licenses' in manifest
  return await resolveRootLicense(manifest, projectDir) ?? (declaresLicense ? undefined : workspaceRootLicense)
}

function serializeSbom (
  result: SbomResult,
  { opts, serialOpts, compact }: { opts: SbomCommandOptions, serialOpts: SerializeOptions, compact: boolean | undefined }
): string {
  if (serialOpts.format !== 'cyclonedx') {
    return serializeSpdx(result, { compact })
  }
  return serializeCycloneDx(result, {
    pnpmVersion: packageManager.version,
    lockfileOnly: opts.lockfileOnly,
    sbomAuthors: opts.sbomAuthors?.split(',').map((author) => author.trim()).filter(Boolean),
    sbomSupplier: opts.sbomSupplier,
    specVersion: serialOpts.sbomSpecVersion,
    compact,
  })
}

/**
 * A selected project the lockfile has no importer for means the lockfile is
 * out of date — pnpm writes an entry for every project, `{}` for one with no
 * dependencies. Walking the rest would answer with an SBOM that under-reports
 * the selection's dependencies, so the run fails instead.
 */
function assertImportersAreInLockfile (lockfile: LockfileObject, importerIds: ProjectId[] | undefined): void {
  if (importerIds == null) return
  const missing = importerIds.filter((importerId) => lockfile.importers[importerId] == null)
  if (missing.length === 0) return
  throw new PnpmError(
    'SBOM_MISSING_IMPORTERS',
    `${WANTED_LOCKFILE} has no entry for the selected project${missing.length === 1 ? '' : 's'}: ${missing.join(', ')}.`,
    { hint: 'Run "pnpm install" to update it.' }
  )
}

/**
 * The value the SBOM's root component publishes for one manifest field. A
 * `--filter` that narrows the run to a single project inherits the workspace
 * root manifest's value for a field that project omits entirely.
 *
 * A field the project declares stays the project's own even when the value
 * names nobody: blank, `null`, or a form no SBOM can publish. The workspace
 * root's author, repository, or issue tracker would attribute the package to
 * the wrong party.
 */
function rootComponentField<FieldName extends keyof ProjectManifest> (
  manifest: ProjectManifest,
  rootManifest: ProjectManifest,
  field: FieldName
): ProjectManifest[FieldName] {
  return field in manifest ? manifest[field] : rootManifest[field]
}

const WORKSPACE_MANIFEST_READ_CONCURRENCY = 8

async function buildWorkspacePackagesMap (
  reachableImporterIds: ProjectId[],
  lockfileDir: string,
  manifestsByImporterId: Map<string, ManifestLike>
): Promise<Record<ProjectId, WorkspacePackageInfo>> {
  if (reachableImporterIds.length === 0) return {} as Record<ProjectId, WorkspacePackageInfo>

  const readManifest = pLimit(WORKSPACE_MANIFEST_READ_CONCURRENCY)
  const entries = await Promise.all(
    reachableImporterIds.map((importerId) => readManifest(async (): Promise<[ProjectId, WorkspacePackageInfo] | null> => {
      const manifest = manifestsByImporterId.get(importerId) ?? await readImporterManifest(lockfileDir, importerId)
      if (!manifest?.name) return null
      return [importerId, toWorkspacePackageInfo(manifest as ManifestLike & { name: string })]
    }))
  )

  return Object.fromEntries(entries.filter((entry) => entry !== null)) as Record<ProjectId, WorkspacePackageInfo>
}

async function readImporterManifest (lockfileDir: string, importerId: ProjectId): Promise<ManifestLike | undefined> {
  if (!isInsideDir(lockfileDir, importerId)) return undefined
  return readManifestSafe(path.join(lockfileDir, importerId))
}

function toWorkspacePackageInfo (manifest: ManifestLike & { name: string }): WorkspacePackageInfo {
  return {
    name: manifest.name,
    version: manifest.version ?? '0.0.0',
    license: typeof manifest.license === 'string' ? manifest.license : undefined,
    description: manifest.description,
    author: authorNameFromField(manifest.author),
    repository: repositoryFromField(manifest.repository),
  }
}

async function readManifestSafe (dir: string): Promise<ManifestLike | undefined> {
  try {
    return await readProjectManifestOnly(dir)
  } catch (err: unknown) {
    if ((err as { code?: string })?.code === 'ERR_PNPM_NO_IMPORTER_MANIFEST_FOUND' || (err as { code?: string })?.code === 'ENOENT') {
      return undefined
    }
    throw err
  }
}

function isInsideDir (parentDir: string, childRelativePath: string): boolean {
  const rel = path.relative(parentDir, path.resolve(parentDir, childRelativePath))
  return rel === '' || (!rel.startsWith('..') && !path.isAbsolute(rel))
}
