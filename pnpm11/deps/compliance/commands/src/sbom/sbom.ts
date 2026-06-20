import fs from 'node:fs'
import { realpath } from 'node:fs/promises'
import path from 'node:path'

import { FILTERING } from '@pnpm/cli.common-cli-options-help'
import { packageManager } from '@pnpm/cli.meta'
import { docsUrl, readProjectManifestOnly } from '@pnpm/cli.utils'
import { type Config, type ConfigContext, types as allTypes } from '@pnpm/config.reader'
import { WANTED_LOCKFILE } from '@pnpm/constants'
import { isSpdxLicenseExpression, resolveLicenseFromDir } from '@pnpm/deps.compliance.license-resolver'
import {
  bugsUrlFromField,
  collectSbomComponents,
  resolveWorkspaceDeps,
  type SbomComponentType,
  type SbomFormat,
  serializeCycloneDx,
  serializeSpdx,
  type WorkspacePackageInfo,
} from '@pnpm/deps.compliance.sbom'
import { PnpmError } from '@pnpm/error'
import { getLockfileImporterId, readWantedLockfile } from '@pnpm/lockfile.fs'
import { getStorePath } from '@pnpm/store.path'
import type { ProjectId, ProjectManifest } from '@pnpm/types'
import { safeReadProjectManifestOnly } from '@pnpm/workspace.project-manifest-reader'
import pLimit from 'p-limit'
import { pick } from 'ramda'
import { renderHelp } from 'render-help'

export type SbomCommandOptions = {
  sbomFormat?: string
  sbomType?: string
  sbomSpecVersion?: string
  lockfileOnly?: boolean
  sbomAuthors?: string
  sbomSupplier?: string
  out?: string
  split?: boolean
  excludePeers?: boolean
} & Pick<
  Config,
  | 'dev'
  | 'dir'
  | 'lockfileDir'
  | 'registries'
  | 'optional'
  | 'production'
  | 'storeDir'
  | 'virtualStoreDir'
  | 'modulesDir'
  | 'pnpmHomeDir'
  | 'virtualStoreDirMaxLength'
> & Pick<ConfigContext,
| 'allProjectsGraph'
| 'selectedProjectsGraph'
| 'rootProjectManifest'
| 'rootProjectManifestDir'
> &
Partial<Pick<Config, 'userConfig'>>

export function rcOptionsTypes (): Record<string, unknown> {
  return pick(
    ['dev', 'global-dir', 'global', 'optional', 'production', 'store-dir'],
    allTypes
  )
}

export const cliOptionsTypes = (): Record<string, unknown> => ({
  ...rcOptionsTypes(),
  recursive: Boolean,
  'sbom-format': String,
  'sbom-type': String,
  'sbom-spec-version': String,
  'sbom-authors': String,
  'sbom-supplier': String,
  'lockfile-only': Boolean,
  out: String,
  split: Boolean,
  'exclude-peers': Boolean,
})

export const shorthands: Record<string, string> = {
  D: '--dev',
  P: '--production',
}

export const commandNames = ['sbom']

export const recursiveByDefault = true

export function help (): string {
  return renderHelp({
    description: 'Generate a Software Bill of Materials (SBOM) for the project.',
    descriptionLists: [
      {
        title: 'Options',
        list: [
          {
            description: 'The SBOM output format (required)',
            name: '--sbom-format <cyclonedx|spdx>',
          },
          {
            description: 'The component type for the root package (default: library)',
            name: '--sbom-type <library|application>',
          },
          {
            description: 'The CycloneDX specification version (1.5, 1.6, or 1.7; default: 1.7)',
            name: '--sbom-spec-version <version>',
          },
          {
            description: 'Only use lockfile data (skip reading from the store)',
            name: '--lockfile-only',
          },
          {
            description: 'Comma-separated list of SBOM authors (CycloneDX metadata.authors)',
            name: '--sbom-authors <names>',
          },
          {
            description: 'SBOM supplier name (CycloneDX metadata.supplier)',
            name: '--sbom-supplier <name>',
          },
          {
            description: 'Only include "dependencies" and "optionalDependencies"',
            name: '--prod',
            shortAlias: '-P',
          },
          {
            description: 'Only include "devDependencies"',
            name: '--dev',
            shortAlias: '-D',
          },
          {
            description: 'Don\'t include "optionalDependencies"',
            name: '--no-optional',
          },
          {
            description: 'Write SBOM to a file instead of stdout. Use %s for the package name and %v for the version.',
            name: '--out <path>',
          },
          {
            description: 'Generate a separate SBOM for each matched workspace package. Outputs NDJSON to stdout, or files when combined with --out.',
            name: '--split',
          },
          {
            description: 'Exclude peer dependencies (and their exclusive transitive subtrees)',
            name: '--exclude-peers',
          },
        ],
      },
      FILTERING,
    ],
    url: docsUrl('sbom'),
    usages: [
      'pnpm sbom --sbom-format cyclonedx',
      'pnpm sbom --sbom-format spdx',
      'pnpm sbom --sbom-format cyclonedx --lockfile-only',
      'pnpm sbom --sbom-format spdx --prod',
      'pnpm sbom --sbom-format cyclonedx --filter ./apps/my-app',
      'pnpm sbom --sbom-format cyclonedx --out out/%s.cdx.json',
      'pnpm sbom --sbom-format cyclonedx --split',
    ],
  })
}

export async function handler (
  opts: SbomCommandOptions,
  _params: string[] = []
): Promise<{ output: string, exitCode: number }> {
  if (!opts.sbomFormat) {
    throw new PnpmError(
      'SBOM_NO_FORMAT',
      'The --sbom-format option is required. Use --sbom-format cyclonedx or --sbom-format spdx.',
      { hint: help() }
    )
  }

  const format = opts.sbomFormat.toLowerCase() as SbomFormat
  if (format !== 'cyclonedx' && format !== 'spdx') {
    throw new PnpmError(
      'SBOM_INVALID_FORMAT',
      `Invalid SBOM format "${opts.sbomFormat}". Use "cyclonedx" or "spdx".`
    )
  }

  const sbomType = validateSbomType(opts.sbomType)
  const sbomSpecVersion = validateSbomSpecVersion(opts.sbomSpecVersion, format)

  const ctx = await buildSharedContext(opts)
  const serialOpts = { format, sbomType, sbomSpecVersion }
  // `%s` in --out only implies per-package output inside a workspace; in a
  // single-project repo it is interpolated from the root component on the
  // single-output path below.
  const hasWorkspaceGraph = opts.selectedProjectsGraph != null || opts.allProjectsGraph != null
  const shouldSplit = opts.split || (opts.out != null && opts.out.includes('%s') && hasWorkspaceGraph)

  if (shouldSplit) {
    return handleSplit(opts, serialOpts, ctx)
  }

  const { output, rootName, rootVersion } = await generateSbomForProject(opts, serialOpts, ctx)

  if (opts.out) {
    const filePath = opts.out
      .replaceAll('%s', sanitizePathSegment(sanitizePackageName(rootName)))
      .replaceAll('%v', sanitizePathSegment(rootVersion))
    fs.mkdirSync(path.dirname(filePath), { recursive: true })
    fs.writeFileSync(filePath, output)
    return { output: filePath, exitCode: 0 }
  }

  return { output, exitCode: 0 }
}

interface SerializeOptions {
  format: SbomFormat
  sbomType: SbomComponentType
  sbomSpecVersion: string | undefined
}

async function handleSplit (
  opts: SbomCommandOptions,
  serialOpts: SerializeOptions,
  ctx: SharedContext
): Promise<{ output: string, exitCode: number }> {
  const projectsGraph = opts.selectedProjectsGraph ?? opts.allProjectsGraph
  if (!projectsGraph) {
    throw new PnpmError(
      'SBOM_NO_PROJECTS',
      'No workspace projects found. --split requires a workspace.'
    )
  }

  if (opts.out && !opts.out.includes('%s')) {
    throw new PnpmError(
      'SBOM_OUT_MISSING_PLACEHOLDER',
      'When using --split with --out, the path must contain %s as a placeholder for the package name.'
    )
  }

  const entries = Object.entries(projectsGraph)
  const ndjsonLines: string[] = []
  const files: string[] = []
  const writtenPaths = new Set<string>()
  const compact = !opts.out
  const createdDirs = new Set<string>()

  for (const [dir, entry] of entries) {
    const manifest = entry.package.manifest
    if (!manifest.name) continue

    const singleProjectGraph = { [dir as keyof typeof projectsGraph]: entry }

    // eslint-disable-next-line no-await-in-loop
    const { output } = await generateSbomForProject(
      { ...opts, selectedProjectsGraph: singleProjectGraph as typeof projectsGraph, allProjectsGraph: undefined, split: false, out: undefined },
      serialOpts,
      ctx,
      compact
    )

    if (opts.out) {
      const filePath = opts.out
        .replaceAll('%s', sanitizePathSegment(sanitizePackageName(manifest.name)))
        .replaceAll('%v', sanitizePathSegment(manifest.version ?? '0.0.0'))
      // Sanitizing package names is lossy (e.g. "@a/b" and "a-b" both map to "a-b"),
      // so two packages can resolve to the same path. Fail loudly instead of letting
      // one SBOM silently overwrite another.
      if (writtenPaths.has(filePath)) {
        throw new PnpmError(
          'SBOM_OUT_PATH_COLLISION',
          `Multiple workspace packages resolve to the same output path "${filePath}". Include %v in the --out pattern to disambiguate.`
        )
      }
      writtenPaths.add(filePath)
      const fileDir = path.dirname(filePath)
      if (!createdDirs.has(fileDir)) {
        fs.mkdirSync(fileDir, { recursive: true })
        createdDirs.add(fileDir)
      }
      fs.writeFileSync(filePath, output)
      files.push(filePath)
    } else {
      ndjsonLines.push(output)
    }
  }

  if (opts.out) {
    return {
      output: `Generated ${files.length} SBOMs:\n${files.map((f) => `  ${f}`).join('\n')}`,
      exitCode: 0,
    }
  }

  return { output: ndjsonLines.join('\n'), exitCode: 0 }
}

type ManifestLike = { name?: string, version?: string, license?: string, description?: string, author?: string | { name?: string }, repository?: string | { url?: string } }

interface SharedContext {
  lockfile: Exclude<Awaited<ReturnType<typeof readWantedLockfile>>, null>
  rootManifest: Awaited<ReturnType<typeof readProjectManifestOnly>>
  rootManifestDir: string
  /** Workspace-root license, resolved once so split mode does not re-probe the
   * root directory as the fallback for every emitted SBOM. */
  rootLicense: string | undefined
  storeDir: string | undefined
  /** Workspace package manifests keyed by lockfile importer id, read once from the
   * project graph so split mode does not re-read them for every emitted SBOM. */
  workspaceManifestsByImporterId: Map<string, ManifestLike>
  excludePeerNamesByImporter?: Map<string, Set<string>>
}

async function buildSharedContext (opts: SbomCommandOptions): Promise<SharedContext> {
  const lockfile = await readWantedLockfile(opts.lockfileDir ?? opts.dir, {
    ignoreIncompatible: true,
  })

  if (lockfile == null) {
    throw new PnpmError(
      'SBOM_NO_LOCKFILE',
      `No ${WANTED_LOCKFILE} found: Cannot generate SBOM without a lockfile`
    )
  }

  const rootManifestDir = opts.rootProjectManifestDir ?? opts.dir
  const rootManifest = opts.rootProjectManifest ?? await readProjectManifestOnly(rootManifestDir)

  const lockfileDir = opts.lockfileDir ?? opts.dir

  // peerDependencies are identified from the manifest, not the lockfile: with
  // auto-install-peers they resolve into the importer's `dependencies` with no
  // distinguishing marker. Map every walked importer to its peer names so the
  // collector can drop them.
  // Keyed by a Map, not a plain object: importer ids come from the lockfile
  // (attacker-controlled in an untrusted clone), and a key like `__proto__`
  // would corrupt a plain object's prototype.
  let excludePeerNamesByImporter: Map<string, Set<string>> | undefined
  if (opts.excludePeers) {
    const byImporter = new Map<string, Set<string>>()
    // Prefer the in-memory project graph(s): no extra filesystem reads, and
    // reading from both graphs (not only the selected subset) covers the extra
    // workspace packages collectSbomComponents reaches through `link:` deps, so
    // their peers are filtered too in a filtered run.
    const graphs = [opts.allProjectsGraph, opts.selectedProjectsGraph].filter(Boolean)
    if (graphs.length > 0) {
      for (const graph of graphs) {
        for (const [projectDir, { package: project }] of Object.entries(graph!)) {
          byImporter.set(getLockfileImporterId(lockfileDir, projectDir), peerNamesFromManifest(project.manifest))
        }
      }
    } else {
      // No project graph (e.g. a single-package repo or `--lockfile-only`
      // outside a workspace), so collectSbomComponents walks every importer in
      // the lockfile. Resolve each importer's own manifest from disk so peers in
      // workspace packages are dropped too, not only those in the directory pnpm
      // ran in. safeReadProjectManifestOnly returns null (rather than throwing)
      // for an importer whose manifest is gone (e.g. a stale lockfile), and skips
      // the installability check that would otherwise abort the SBOM.
      const lockfileRoot = await realpath(lockfileDir)
      // Bound the fan-out: a large workspace can have many importers, and
      // reading every manifest at once would spike open file descriptors.
      const limitManifestReads = pLimit(16)
      await Promise.all(
        Object.keys(lockfile.importers).map((importerId) => limitManifestReads(async () => {
          // A crafted lockfile could carry an importer key that escapes the
          // project, via `..` segments or a symlinked directory. Canonicalize
          // with realpath and skip anything resolving outside the project root,
          // so a manifest is never read from outside the tree.
          let importerDir: string
          try {
            importerDir = await realpath(path.resolve(lockfileDir, importerId))
          } catch {
            return
          }
          const rel = path.relative(lockfileRoot, importerDir)
          if (rel !== '' && (rel.startsWith('..') || path.isAbsolute(rel))) return
          // safeReadProjectManifestOnly tolerates a missing manifest but still
          // throws on a malformed one (parse error). In an untrusted clone a
          // single junk package.json must not abort the whole SBOM, so skip it
          // (fail-open: an unparseable importer's peers just aren't filtered).
          let importerManifest: ProjectManifest | null
          try {
            importerManifest = await safeReadProjectManifestOnly(importerDir)
          } catch {
            return
          }
          if (importerManifest) {
            byImporter.set(importerId, peerNamesFromManifest(importerManifest))
          }
        }))
      )
    }
    excludePeerNamesByImporter = byImporter
  }

  let storeDir: string | undefined
  if (!opts.lockfileOnly) {
    storeDir = await getStorePath({
      pkgRoot: opts.dir,
      storePath: opts.storeDir,
      pnpmHomeDir: opts.pnpmHomeDir,
    })
  }

  const workspaceManifestsByImporterId = new Map<string, ManifestLike>()
  for (const graph of [opts.allProjectsGraph, opts.selectedProjectsGraph]) {
    if (!graph) continue
    for (const [dir, entry] of Object.entries(graph)) {
      workspaceManifestsByImporterId.set(getLockfileImporterId(lockfileDir, dir), entry.package.manifest)
    }
  }

  const rootLicense = await resolveRootLicense(rootManifest, rootManifestDir)

  return { lockfile, rootManifest, rootManifestDir, rootLicense, storeDir, workspaceManifestsByImporterId, excludePeerNamesByImporter }
}

async function generateSbomForProject (
  opts: SbomCommandOptions,
  serialOpts: SerializeOptions,
  ctx: SharedContext,
  compact?: boolean
): Promise<{ output: string, exitCode: number, rootName: string, rootVersion: string }> {
  const { lockfile, rootManifest, rootManifestDir, rootLicense: cachedRootLicense } = ctx

  const include = {
    dependencies: opts.production !== false,
    devDependencies: opts.dev !== false,
    optionalDependencies: opts.optional !== false,
  }

  const selectedEntries = opts.selectedProjectsGraph
    ? Object.entries(opts.selectedProjectsGraph)
    : undefined
  const singleProject = selectedEntries?.length === 1
    ? selectedEntries[0]
    : undefined

  const manifest = singleProject
    ? singleProject[1].package.manifest
    : rootManifest
  const projectDir = singleProject
    ? singleProject[0]
    : rootManifestDir

  const rootName = manifest.name ?? 'unknown'
  const rootVersion = manifest.version ?? '0.0.0'
  const rootLicense = singleProject
    ? (await resolveRootLicense(manifest, projectDir) ?? cachedRootLicense)
    : cachedRootLicense
  const rootAuthor = extractAuthor(manifest)
    ?? (singleProject ? extractAuthor(rootManifest) : undefined)
  const rootRepository = extractRepository(manifest)
    ?? (singleProject ? extractRepository(rootManifest) : undefined)
  const rootDescription = manifest.description
    ?? (singleProject ? rootManifest.description : undefined)
  const rootBugsUrl = bugsUrlFromField(manifest.bugs)
    ?? (singleProject ? bugsUrlFromField(rootManifest.bugs) : undefined)

  const lockfileDir = opts.lockfileDir ?? opts.dir
  const includedImporterIds = opts.selectedProjectsGraph
    ? Object.keys(opts.selectedProjectsGraph)
      .map((p) => getLockfileImporterId(lockfileDir, p))
    : undefined

  const resolvedWorkspaceDeps = opts.lockfileOnly
    ? undefined
    : resolveWorkspaceDeps(
      lockfile,
      includedImporterIds ?? Object.keys(lockfile.importers) as ProjectId[],
      include
    )
  const workspacePackages = resolvedWorkspaceDeps
    ? await buildWorkspacePackagesMap(
      resolvedWorkspaceDeps.additionalImporterIds,
      lockfileDir,
      ctx.workspaceManifestsByImporterId
    )
    : undefined

  const result = await collectSbomComponents({
    lockfile,
    rootName,
    rootVersion,
    rootLicense,
    rootDescription,
    rootAuthor,
    rootRepository,
    rootBugsUrl,
    sbomType: serialOpts.sbomType,
    include,
    registries: opts.registries,
    lockfileDir,
    includedImporterIds,
    lockfileOnly: opts.lockfileOnly,
    storeDir: ctx.storeDir,
    virtualStoreDirMaxLength: opts.virtualStoreDirMaxLength,
    workspacePackages,
    resolvedWorkspaceDeps,
    excludePeerNamesByImporter: ctx.excludePeerNamesByImporter,
  })

  const output = serialOpts.format === 'cyclonedx'
    ? serializeCycloneDx(result, {
      pnpmVersion: packageManager.version,
      lockfileOnly: opts.lockfileOnly,
      sbomAuthors: opts.sbomAuthors?.split(',').map((s) => s.trim()).filter(Boolean),
      sbomSupplier: opts.sbomSupplier,
      specVersion: serialOpts.sbomSpecVersion,
      compact,
    })
    : serializeSpdx(result, { compact })

  return { output, exitCode: 0, rootName, rootVersion }
}

function peerNamesFromManifest (manifest: ProjectManifest): Set<string> {
  // A name declared as both a peer and a regular dependency is a real dependency
  // the package pulls in itself, so keep it.
  const regular = new Set([
    ...Object.keys(manifest.dependencies ?? {}),
    ...Object.keys(manifest.devDependencies ?? {}),
    ...Object.keys(manifest.optionalDependencies ?? {}),
  ])
  return new Set(
    Object.keys(manifest.peerDependencies ?? {}).filter((name) => !regular.has(name))
  )
}

function validateSbomType (value: string | undefined): SbomComponentType {
  if (!value || value === 'library') return 'library'
  if (value === 'application') return 'application'
  throw new PnpmError(
    'SBOM_INVALID_TYPE',
    `Invalid SBOM type "${value}". Use "library" or "application".`
  )
}

const SUPPORTED_CYCLONEDX_SPEC_VERSIONS = ['1.5', '1.6', '1.7']

function validateSbomSpecVersion (value: string | undefined, format: SbomFormat): string | undefined {
  if (value == null) return undefined
  if (format !== 'cyclonedx') {
    throw new PnpmError(
      'SBOM_SPEC_VERSION_UNSUPPORTED_FORMAT',
      'The --sbom-spec-version option is only supported with --sbom-format cyclonedx.'
    )
  }
  const normalized = value.trim()
  if (!SUPPORTED_CYCLONEDX_SPEC_VERSIONS.includes(normalized)) {
    throw new PnpmError(
      'SBOM_INVALID_SPEC_VERSION',
      `Invalid CycloneDX spec version "${value}". Supported versions: ${SUPPORTED_CYCLONEDX_SPEC_VERSIONS.join(', ')}.`
    )
  }
  return normalized
}

async function resolveRootLicense (manifest: Parameters<typeof resolveLicenseFromDir>[0]['manifest'], dir: string): Promise<string | undefined> {
  // Skip the on-disk LICENSE probing when the manifest already declares a usable
  // SPDX license; resolveLicenseFromDir would return the same value after the scan.
  if (typeof manifest.license === 'string' && isSpdxLicenseExpression(manifest.license)) {
    return manifest.license
  }
  const info = await resolveLicenseFromDir({ manifest, dir })
  if (info && info.name !== 'Unknown' && (!info.licenseFile || isSpdxLicenseExpression(info.name))) {
    return info.name
  }
  return undefined
}

function extractAuthor (manifest: { author?: string | { name?: string } }): string | undefined {
  if (typeof manifest.author === 'string') return manifest.author
  return manifest.author?.name
}

function extractRepository (manifest: { repository?: string | { url?: string } }): string | undefined {
  if (typeof manifest.repository === 'string') return manifest.repository
  return manifest.repository?.url
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
      const known = manifestsByImporterId.get(importerId)
      const manifest = known
        ? known
        : isInsideDir(lockfileDir, importerId)
          ? await readManifestSafe(path.join(lockfileDir, importerId))
          : undefined

      if (!manifest?.name) return null

      return [importerId, {
        name: manifest.name,
        version: manifest.version ?? '0.0.0',
        license: typeof manifest.license === 'string' ? manifest.license : undefined,
        description: manifest.description,
        author: extractAuthor(manifest),
        repository: extractRepository(manifest),
      }]
    }))
  )

  return Object.fromEntries(entries.filter((e) => e !== null)) as Record<ProjectId, WorkspacePackageInfo>
}

async function readManifestSafe (dir: string): Promise<ManifestLike | undefined> {
  try {
    return await readProjectManifestOnly(dir)
  } catch {
    return undefined
  }
}

function sanitizePackageName (name: string): string {
  return name.replace(/^@/, '').replace(/\//g, '-')
}

function sanitizePathSegment (value: string): string {
  // Control characters (e.g. newlines) would produce confusing filenames and could
  // inject extra lines into the printed `--split --out` summary; filesystem
  // metacharacters are replaced so the value stays a single path segment.
  // eslint-disable-next-line no-control-regex
  const sanitized = value.replace(/[/\\:*?"<>|\x00-\x1F\x7F]/g, '-')
  // `.`, `..`, or a blank value would let a crafted name/version escape or replace
  // the intended output directory once interpolated into an `--out` template.
  return sanitized === '.' || sanitized === '..' || sanitized.trim() === '' ? '-' : sanitized
}

function isInsideDir (parentDir: string, childRelativePath: string): boolean {
  const rel = path.relative(parentDir, path.resolve(parentDir, childRelativePath))
  return rel === '' || (!rel.startsWith('..') && !path.isAbsolute(rel))
}
