import fs from 'node:fs'
import path from 'node:path'

import { getBinsFromPackageManifest } from '@pnpm/bins.resolver'
import type { Catalogs } from '@pnpm/catalogs.types'
import { readProjectManifest } from '@pnpm/cli.utils'
import type { Config } from '@pnpm/config.reader'
import { PnpmError } from '@pnpm/error'
import { packlistWithSources } from '@pnpm/fs.packlist'
import type { Hooks } from '@pnpm/hooks.pnpmfile'
import { createExportableManifest, type ExportedManifest, readReadmeFile, type WorkspacePackageLookup } from '@pnpm/releasing.exportable-manifest'
import { changelogStorage, readPendingChangelog, renderChangelog } from '@pnpm/releasing.versioning'
import type { DependencyManifest, ProjectManifest } from '@pnpm/types'
import { glob } from 'tinyglobby'
import validateNpmPackageName from 'validate-npm-package-name'

import { normalizePackageName } from '../tarball/safeTarballFilename.js'
import type { PackOptions, PackResultJson } from './pack.js'
import { checkPackedBinsForCrlf, isManifestEntry, packPkg } from './packTarball.js'
import { fetchPreviousChangelog, type PreviousChangelogOptions } from './previousChangelog.js'
import { bindRunScriptsIfPresent } from './publish.js'

const LICENSE_GLOB = 'LICEN{S,C}E{,.*}' // cspell:disable-line

export interface PackResult {
  publishedManifest: ExportedManifest
  contents: string[]
  tarballPath: string
  /** Total uncompressed size of all files in the tarball, in bytes. */
  unpackedSize: number
}

export async function api (opts: PackOptions): Promise<PackResult> {
  const { manifest: entryManifest, fileName: manifestFileName } = await readProjectManifest(opts.dir, opts)
  preventBundledDependenciesWithPnpNodeLinker(opts.nodeLinker, entryManifest)
  const _runScriptsIfPresent = await bindRunScriptsIfPresent(opts.dir, opts)
  if (!opts.ignoreScripts) {
    await _runScriptsIfPresent([
      'prepack',
      'prepare',
    ], entryManifest)
  }
  const dir = entryManifest.publishConfig?.directory
    ? path.join(opts.dir, entryManifest.publishConfig.directory)
    : opts.dir
  // always read the latest manifest, as "prepack" or "prepare" script may modify package manifest.
  const { manifest, fileName: selectedManifestFileName } = await readProjectManifest(dir, opts)
  preventBundledDependenciesWithPnpNodeLinker(opts.nodeLinker, manifest)
  assertPackableManifest(manifest, manifestFileName)
  return packProject({
    opts,
    dir,
    manifest,
    selectedManifestFileName,
    runPostpack: async () => {
      if (!opts.ignoreScripts) {
        await _runScriptsIfPresent(['postpack'], entryManifest)
      }
    },
  })
}

type PackableManifest = ProjectManifest & { name: string, version: string }

function assertPackableManifest (manifest: ProjectManifest, manifestFileName: string): asserts manifest is PackableManifest {
  if (!manifest.name) {
    throw new PnpmError('PACKAGE_NAME_NOT_FOUND', `Package name is not defined in the ${manifestFileName}.`)
  }
  if (!validateNpmPackageName(manifest.name).validForOldPackages) {
    throw new PnpmError('INVALID_PACKAGE_NAME', `Invalid package name "${manifest.name}".`)
  }
  if (!manifest.version) {
    throw new PnpmError('PACKAGE_VERSION_NOT_FOUND', `Package version is not defined in the ${manifestFileName}.`)
  }
}

interface PackProjectOptions {
  opts: PackOptions
  /** The directory that is packed: the project directory or its `publishConfig.directory`. */
  dir: string
  manifest: PackableManifest
  selectedManifestFileName: string
  runPostpack: () => Promise<void>
}

async function packProject ({ opts, dir, manifest, selectedManifestFileName, runPostpack }: PackProjectOptions): Promise<PackResult> {
  const { publishManifest, publishedName } = await createPackedManifest(opts, dir, manifest)
  const { destDir, outputPath, tarballName } = resolvePackOutput({
    dir,
    out: opts.out,
    packDestination: opts.packDestination,
    publishedName,
    publishedVersion: publishManifest.version!,
  })
  const { filesMap, binPaths } = await collectPackedFiles({ opts, dir, manifest, publishManifest, selectedManifestFileName })
  const injectedEntries = await collectInjectedEntries({ opts, manifest, publishedName, filesMap })
  if (!opts.dryRun) {
    await fs.promises.mkdir(destDir, { recursive: true })
  }
  const { unpackedSize, packedContents } = await summarizePackedEntries(filesMap, injectedEntries, publishManifest)
  if (!opts.dryRun) {
    const packAndRunPostpack = async (): Promise<void> => {
      await packPkg({
        destFile: outputPath,
        filesMap,
        injectedEntries,
        modulesDir: path.join(opts.dir, 'node_modules'),
        packGzipLevel: opts.packGzipLevel,
        manifest: publishManifest,
        bins: binPaths,
      })
      await runPostpack()
    }
    await lockPackDestination(opts, path.resolve(outputPath), packAndRunPostpack)
  }
  return {
    publishedManifest: await withRegistryReadme(publishManifest, dir),
    contents: packedContents,
    tarballPath: opts.dir !== destDir
      ? path.join(destDir, tarballName)
      : path.relative(opts.dir, path.join(dir, tarballName)),
    unpackedSize,
  }
}

async function lockPackDestination (opts: PackOptions, destination: string, write: () => Promise<void>): Promise<void> {
  if (opts.packDestinationLocker == null) {
    await write()
  } else {
    await opts.packDestinationLocker(destination, write)
  }
}

async function createPackedManifest (
  opts: PackOptions,
  dir: string,
  manifest: PackableManifest
): Promise<{ publishManifest: ExportedManifest, publishedName: string }> {
  const publishManifest = await createPublishManifest({
    projectDir: dir,
    modulesDir: path.join(opts.dir, 'node_modules'),
    manifest,
    embedReadme: opts.embedReadme,
    catalogs: opts.catalogs ?? {},
    hooks: opts.hooks,
    skipManifestObfuscation: opts.skipManifestObfuscation,
    workspacePackages: await findWorkspacePackages(opts),
  })
  // Strip semver build metadata (the `+<build>` segment) from the published version so that
  // the tarball, the manifest packed inside it, and the metadata sent to the registry all agree.
  // libnpmpublish runs `semver.clean()` on `manifest.version` before computing the provenance
  // subject, which removes build metadata. Leaving it in here would mismatch the version embedded
  // in the tarball's package.json and cause the registry to reject the publish with a 422 when
  // verifying the sigstore provenance bundle. See https://github.com/pnpm/pnpm/issues/11518.
  publishManifest.version = stripBuildMetadata(publishManifest.version!)
  // Read back off the publish manifest so a `publishConfig.name` rename reaches
  // the filename too — the tarball name, the packed manifest, and the registry
  // metadata all name one artifact. The rename never went through the check on
  // `manifest.name` above, so it is validated here: it lands in the tarball
  // filename, where a separator would smuggle path components into the join and
  // write outside the pack destination.
  const publishedName = publishManifest.name || manifest.name
  if (!validateNpmPackageName(publishedName).validForOldPackages) {
    throw new PnpmError('INVALID_PACKAGE_NAME', `Invalid package name "${publishedName}".`)
  }
  return { publishManifest, publishedName }
}

async function findWorkspacePackages (opts: PackOptions): Promise<WorkspacePackageLookup | undefined> {
  const workspacePackages: WorkspacePackageLookup | undefined = opts.allProjects ??
    (opts.selectedProjectsGraph ? Object.values(opts.selectedProjectsGraph).map((p) => p.package) : undefined) ??
    (opts.allProjectsGraph ? Object.values(opts.allProjectsGraph).map((p) => p.package) : undefined)
  if (workspacePackages || !opts.workspaceDir) return workspacePackages
  const { filterProjectsBySelectorObjectsFromDir } = await import('@pnpm/workspace.projects-filter')
  const result = await filterProjectsBySelectorObjectsFromDir(opts.workspaceDir, [])
  return result.allProjects
}

interface CollectPackedFilesOptions {
  opts: PackOptions
  dir: string
  manifest: PackableManifest
  publishManifest: ExportedManifest
  selectedManifestFileName: string
}

interface PackedFiles {
  /** Tar entry names (`package/<path>`) mapped to the files they are packed from. */
  filesMap: Record<string, string>
  binPaths: string[]
}

async function collectPackedFiles ({ opts, dir, manifest, publishManifest, selectedManifestFileName }: CollectPackedFilesOptions): Promise<PackedFiles> {
  const sources = await packlistWithSources(dir, {
    manifest: publishManifest as Record<string, unknown>,
    workspaceDir: opts.workspaceDir,
    bundledDependenciesDir: opts.dir,
  })
  const files = Array.from(sources.keys())
  const filesMap = Object.fromEntries(Array.from(sources, ([file, source]) => [`package/${file}`, source]))
  for (const name of Object.keys(filesMap)) {
    if (isManifestEntry(name)) delete filesMap[name]
  }
  filesMap['package/package.json'] = path.join(dir, selectedManifestFileName)
  const binPaths = [
    ...(await getBinsFromPackageManifest(publishManifest as DependencyManifest, dir)).map(({ path }) => path),
    ...(manifest.publishConfig?.executableFiles ?? [])
      .map((executableFile) => path.join(dir, executableFile)),
  ]
  await checkPackedBinsForCrlf(filesMap, binPaths)
  await injectWorkspaceLicense({ filesMap, files, dir, workspaceDir: opts.workspaceDir })
  return { filesMap, binPaths }
}

async function injectWorkspaceLicense (
  { filesMap, files, dir, workspaceDir }: { filesMap: Record<string, string>, files: string[], dir: string, workspaceDir?: string }
): Promise<void> {
  // cspell:disable-next-line
  if (workspaceDir == null || dir === workspaceDir || files.some((file) => /^LICEN[CS]E(?:\..+)?$/i.test(path.basename(file)))) return
  const licenses = await glob([LICENSE_GLOB], { cwd: workspaceDir, expandDirectories: false })
  await Promise.all(licenses.map(async (license) => {
    const licensePath = path.join(workspaceDir, license)
    // Only inject a regular file. A symlink could point outside the workspace and leak its
    // target's bytes into the published tarball, so `lstat()` (which does not follow symlinks)
    // rejects it.
    const stats = await fs.promises.lstat(licensePath)
    if (stats.isFile()) {
      filesMap[`package/${license}`] = licensePath
    }
  }))
}

/**
 * In `registry` changelog storage the package carries no committed
 * CHANGELOG.md; its section was parked at `pnpm version -r` time and is
 * composed here on top of the previously published version's changelog and
 * packed in. A composed entry supersedes any stale committed CHANGELOG.md.
 */
async function collectInjectedEntries (
  { opts, manifest, publishedName, filesMap }: { opts: PackOptions, manifest: PackableManifest, publishedName: string, filesMap: Record<string, string> }
): Promise<Record<string, string>> {
  const injectedEntries: Record<string, string> = {}
  const composedChangelog = await composeRegistryChangelog(opts, manifest.name, publishedName, manifest.version)
  if (composedChangelog != null) {
    delete filesMap['package/CHANGELOG.md']
    injectedEntries['package/CHANGELOG.md'] = composedChangelog
  }
  return injectedEntries
}

/**
 * Derive `contents` and `unpackedSize` from `filesMap` (the full set of tar entries) rather than
 * from the packlist subset so that:
 *   - workspace LICENSE files appended to `filesMap` after the packlist call are included; and
 *   - `package.yaml` / `package.json5` entries are reported under the name they actually have in
 *     the tar (`package.json`), since `packPkg()` rewrites them.
 * The `stat()` pass must run before `postpack`, which may delete prepack-generated files that
 * were packed. See https://github.com/pnpm/pnpm/issues/12775.
 */
async function summarizePackedEntries (
  filesMap: Record<string, string>,
  injectedEntries: Record<string, string>,
  publishManifest: ExportedManifest
): Promise<{ unpackedSize: number, packedContents: string[] }> {
  const sizes = await Promise.all(Object.entries(filesMap).map(async ([name, source]) => {
    if (isManifestEntry(name)) {
      return Buffer.byteLength(JSON.stringify(publishManifest, null, 2))
    }
    const stat = await fs.promises.lstat(source)
    return stat.isSymbolicLink() ? 0 : stat.size
  }))
  const injectedSize = Object.values(injectedEntries).reduce((acc, content) => acc + Buffer.byteLength(content), 0)
  const unpackedSize = sizes.reduce((acc, size) => acc + size, 0) + injectedSize
  const packedContents = Array.from(new Set([
    ...Object.keys(filesMap).map((name) =>
      isManifestEntry(name)
        ? 'package.json'
        : name.replace(/^package\//, '')
    ),
    ...Object.keys(injectedEntries).map((name) => name.replace(/^package\//, '')),
  ])).sort((a, b) => a.localeCompare(b, 'en'))
  return { unpackedSize, packedContents }
}

export function resolvePackOutput (
  params: {
    dir: string
    out?: string
    packDestination?: string
    publishedName: string
    publishedVersion: string
  }
): { destDir: string, outputPath: string, tarballName: string } {
  const { dir, out, packDestination, publishedName } = params
  const normalizedName = normalizePackageName(publishedName)
  const publishedVersion = stripBuildMetadata(params.publishedVersion)
  let tarballName: string
  let destination: string | undefined
  if (out) {
    if (packDestination) {
      throw new PnpmError('INVALID_OPTION', 'Cannot use --pack-destination and --out together')
    }
    const preparedOut = out.replaceAll('%s', normalizedName).replaceAll('%v', publishedVersion)
    const parsedOut = path.parse(preparedOut)
    destination = parsedOut.dir || packDestination
    tarballName = parsedOut.base
  } else {
    destination = packDestination
    tarballName = `${normalizedName}-${publishedVersion}.tgz`
  }
  const destDir = destination
    ? (path.isAbsolute(destination) ? destination : path.join(dir, destination))
    : dir
  return { destDir, outputPath: path.join(destDir, tarballName), tarballName }
}

/**
 * The readme is always sent to the registry as package metadata, matching the npm CLI, so that
 * registries can render it on the package page. The `embed-readme` setting only controls whether
 * the readme is additionally written into the `package.json` inside the tarball (via
 * `createExportableManifest`), which is why it is added to the returned manifest here rather than
 * to the packed one.
 */
async function withRegistryReadme (manifest: ExportedManifest, projectDir: string): Promise<ExportedManifest> {
  if (manifest.readme != null) return manifest
  const readme = await readReadmeFile(projectDir)
  if (readme == null) return manifest
  return { ...manifest, readme }
}

/**
 * The CHANGELOG.md to pack for a `registry`-storage release: its parked
 * section (written at `pnpm version -r` time) rendered on top of the
 * previously published version's changelog. `undefined` when storage is
 * `repository`, there is no workspace, or the release has no parked section
 * (an ordinary `pnpm pack` of a package that is not mid-release).
 */
/**
 * `pkgName` keys the parked section — the workspace, the ledger, and every
 * intent address the project by its manifest name. `publishedName` is the only
 * name the registry knows, so it selects the previous changelog to build on and
 * titles the composed one.
 */
async function composeRegistryChangelog (opts: PackOptions, pkgName: string, publishedName: string, version: string): Promise<string | undefined> {
  if (changelogStorage(opts.versioning) !== 'registry' || opts.workspaceDir == null) return undefined
  const section = await readPendingChangelog(opts.workspaceDir, pkgName, version)
  if (section == null) return undefined
  const previous = opts.registriesByScope != null
    ? await fetchPreviousChangelog(opts as PreviousChangelogOptions, publishedName, version)
    : undefined
  return renderChangelog(previous ?? null, publishedName, section)
}

function stripBuildMetadata (version: string): string {
  const plusIndex = version.indexOf('+')
  return plusIndex === -1 ? version : version.slice(0, plusIndex)
}

function preventBundledDependenciesWithPnpNodeLinker (nodeLinker: Config['nodeLinker'], manifest: ProjectManifest): void {
  if (nodeLinker !== 'pnp') return
  for (const key of ['bundledDependencies', 'bundleDependencies'] as const) {
    const bundledDependencies = manifest[key]
    if (bundledDependencies) {
      throw new PnpmError('BUNDLED_DEPENDENCIES_WITHOUT_HOISTED', `${key} does not work with "nodeLinker: ${nodeLinker}"`, {
        hint: `Set "nodeLinker: isolated" or "nodeLinker: hoisted" in pnpm-workspace.yaml or delete ${key} from the root package.json to resolve this error`,
      })
    }
  }
}

async function createPublishManifest (opts: {
  projectDir: string
  embedReadme?: boolean
  modulesDir: string
  manifest: ProjectManifest
  catalogs: Catalogs
  hooks?: Hooks
  skipManifestObfuscation?: boolean
  workspacePackages?: WorkspacePackageLookup
}): Promise<ExportedManifest> {
  const { projectDir, embedReadme, modulesDir, manifest, catalogs, hooks, skipManifestObfuscation, workspacePackages } = opts
  return createExportableManifest(projectDir, manifest, {
    catalogs,
    hooks,
    embedReadme,
    modulesDir,
    skipManifestObfuscation,
    workspacePackages,
  })
}

export function toPackResultJson (packResult: PackResult): PackResultJson {
  const { publishedManifest, contents, tarballPath } = packResult
  return {
    name: publishedManifest.name as string,
    version: publishedManifest.version as string,
    filename: tarballPath,
    files: contents.map((file) => ({ path: file })),
  }
}
