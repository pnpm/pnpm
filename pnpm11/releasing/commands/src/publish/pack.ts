import fs from 'node:fs'
import path from 'node:path'
import { createGzip } from 'node:zlib'

import { getBinsFromPackageManifest } from '@pnpm/bins.resolver'
import type { Catalogs } from '@pnpm/catalogs.types'
import { FILTERING } from '@pnpm/cli.common-cli-options-help'
import { readProjectManifest } from '@pnpm/cli.utils'
import { type Config, type ConfigContext, getDefaultWorkspaceConcurrency, getWorkspaceConcurrency, types as allTypes, type UniversalOptions } from '@pnpm/config.reader'
import { PnpmError } from '@pnpm/error'
import { packlist } from '@pnpm/fs.packlist'
import type { Hooks } from '@pnpm/hooks.pnpmfile'
import { logger } from '@pnpm/logger'
import { createExportableManifest, type ExportedManifest, readReadmeFile } from '@pnpm/releasing.exportable-manifest'
import { changelogStorage, readPendingChangelog, renderChangelog } from '@pnpm/releasing.versioning'
import type { DependencyManifest, Project, ProjectManifest, ProjectRootDir, ProjectsGraph } from '@pnpm/types'
import { sortFilteredProjects } from '@pnpm/workspace.projects-sorter'
import chalk from 'chalk'
import pLimit from 'p-limit'
import { pick } from 'ramda'
import { realpathMissing } from 'realpath-missing'
import { renderHelp } from 'render-help'
import tar from 'tar-stream'
import { glob } from 'tinyglobby'
import validateNpmPackageName from 'validate-npm-package-name'

import { fetchPreviousChangelog, type PreviousChangelogOptions } from './previousChangelog.js'
import { runScriptsIfPresent } from './publish.js'

const LICENSE_GLOB = 'LICEN{S,C}E{,.*}' // cspell:disable-line

export function rcOptionsTypes (): Record<string, unknown> {
  return {
    ...cliOptionsTypes(),
    ...pick([
      'npm-path',
      'skip-manifest-obfuscation',
    ], allTypes),
  }
}

export function cliOptionsTypes (): Record<string, unknown> {
  return {
    out: String,
    recursive: Boolean,
    ...pick([
      'dry-run',
      'ignore-scripts',
      'pack-destination',
      'pack-gzip-level',
      'json',
      'skip-manifest-obfuscation',
      'workspace-concurrency',
    ], allTypes),
  }
}

export const commandNames = ['pack']

export function help (): string {
  return renderHelp({
    description: 'Create a tarball from a package',
    usages: ['pnpm pack'],
    descriptionLists: [
      {
        title: 'Options',

        list: [
          {
            description: 'Does everything `pnpm pack` would do except actually writing the tarball to disk.',
            name: '--dry-run',
          },
          {
            description: 'Directory in which `pnpm pack` will save tarballs. The default is the current working directory.',
            name: '--pack-destination <dir>',
          },
          {
            description: 'Does not run the `prepack`, `prepare` and `postpack` lifecycle scripts. Combined with `--dry-run --json`, this prints the manifest that would be published without building the package.',
            name: '--ignore-scripts',
          },
          {
            description: 'Prints the packed tarball, its contents and the manifest that goes into it in the json format.',
            name: '--json',
          },
          {
            description: 'Customizes the output path for the tarball. Use `%s` and `%v` to include the package name and version, e.g., `%s.tgz` or `some-dir/%s-%v.tgz`. By default, the tarball is saved in the current working directory with the name `<package-name>-<version>.tgz`.',
            name: '--out <path>',
          },
          {
            description: 'Pack all packages from the workspace',
            name: '--recursive',
            shortAlias: '-r',
          },
          {
            description: 'Skip pnpm\'s manifest obfuscation: keep the original `packageManager` field and publish lifecycle scripts in the packed manifest instead of stripping them. The pnpm-specific `pnpm` field is still omitted.',
            name: '--skip-manifest-obfuscation',
          },
          {
            description: `Set the maximum number of concurrency. Default is ${getDefaultWorkspaceConcurrency()}. For unlimited concurrency use Infinity.`,
            name: '--workspace-concurrency <number>',
          },
        ],
      },
      FILTERING,
    ],
  })
}

export type PackOptions = Pick<UniversalOptions, 'dir'> & Pick<Config, 'catalogs'
| 'ignoreScripts'
| 'embedReadme'
| 'packGzipLevel'
| 'nodeLinker'
| 'skipManifestObfuscation'
| 'userAgent'
> & Partial<Pick<Config, 'extraBinPaths'
| 'extraEnv'
| 'recursive'
| 'workspaceConcurrency'
| 'workspaceDir'
// Registry-storage changelog composition (see `injectChangelog`): the
// registry to read the previous version's tarball from, plus the network
// config `createFetchFromRegistry` needs.
| 'versioning'
| 'registries'
| 'configByUri'
| 'fetchRetries'
| 'fetchRetryFactor'
| 'fetchRetryMaxtimeout'
| 'fetchRetryMintimeout'
| 'fetchTimeout'
| 'ca'
| 'cert'
| 'key'
| 'strictSsl'
| 'httpProxy'
| 'httpsProxy'
| 'noProxy'
| 'localAddress'
>> & Partial<Pick<ConfigContext,
| 'hooks'
| 'selectedProjectsGraph'
| 'allProjectsGraph'
| 'prodAllProjectsGraph'
| 'prodOnlySelectedProjectDirs'
>> & {
  argv: {
    original: string[]
  }
  dryRun?: boolean
  engineStrict?: boolean
  packDestination?: string
  out?: string
  json?: boolean
  unicode?: boolean
}

export interface PackResultJson {
  name: string
  version: string
  filename: string
  files: Array<{ path: string }>
  manifest: ExportedManifest
}

export async function handler (opts: PackOptions): Promise<string> {
  const packedPackages: PackResultJson[] = []

  if (opts.recursive) {
    const selectedProjectsGraph = opts.selectedProjectsGraph as ProjectsGraph
    const pkgsToPack: Project[] = []
    for (const { package: pkg } of Object.values(selectedProjectsGraph)) {
      if (pkg.manifest.name && pkg.manifest.version) {
        pkgsToPack.push(pkg)
      }
    }
    const packedPkgDirs = new Set<ProjectRootDir>(pkgsToPack.map(({ rootDir }) => rootDir))

    if (packedPkgDirs.size === 0) {
      logger.info({
        message: 'There are no packages that should be packed',
        prefix: opts.dir,
      })
    }

    const chunks = sortFilteredProjects({
      selectedProjectsGraph,
      allProjectsGraph: opts.allProjectsGraph,
      prodAllProjectsGraph: opts.prodAllProjectsGraph,
      prodOnlySelectedProjectDirs: opts.prodOnlySelectedProjectDirs,
    })

    const limitPack = pLimit(getWorkspaceConcurrency(opts.workspaceConcurrency))
    const resolvedOpts = { ...opts }
    if (opts.out) {
      resolvedOpts.out = path.resolve(opts.dir, opts.out)
    } else if (opts.packDestination) {
      resolvedOpts.packDestination = path.resolve(opts.dir, opts.packDestination)
    } else {
      resolvedOpts.packDestination = path.resolve(opts.dir)
    }
    for (const chunk of chunks) {
      // eslint-disable-next-line no-await-in-loop
      await Promise.all(chunk.map(pkgDir =>
        limitPack(async () => {
          if (!packedPkgDirs.has(pkgDir)) return
          const pkg = selectedProjectsGraph[pkgDir].package
          const packResult = await api({
            ...resolvedOpts,
            dir: pkg.rootDir,
          })
          packedPackages.push(toPackResultJson(packResult))
        })
      ))
    }
  } else {
    const packResult = await api(opts)
    packedPackages.push(toPackResultJson(packResult))
  }

  if (opts.json) {
    return JSON.stringify(packedPackages.length > 1 ? packedPackages : packedPackages[0], null, 2)
  }

  return packedPackages.map(
    ({ name, version, filename, files }) => `${opts.unicode ? '📦 ' : 'package:'} ${name}@${version}
${chalk.blueBright('Tarball Contents')}
${files.map(({ path }) => path).join('\n')}
${chalk.blueBright('Tarball Details')}
${filename}`
  ).join('\n\n')
}

export async function api (opts: PackOptions): Promise<PackResult> {
  const { manifest: entryManifest, fileName: manifestFileName } = await readProjectManifest(opts.dir, opts)
  preventBundledDependenciesWithoutHoistedNodeLinker(opts.nodeLinker, entryManifest)
  const _runScriptsIfPresent = runScriptsIfPresent.bind(null, {
    depPath: opts.dir,
    extraBinPaths: opts.extraBinPaths,
    extraEnv: opts.extraEnv,
    pkgRoot: opts.dir,
    rootModulesDir: await realpathMissing(path.join(opts.dir, 'node_modules')),
    stdio: 'inherit',
    unsafePerm: true, // when running scripts explicitly, assume that they're trusted.
    userAgent: opts.userAgent,
  })
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
  const { manifest } = await readProjectManifest(dir, opts)
  preventBundledDependenciesWithoutHoistedNodeLinker(opts.nodeLinker, manifest)
  if (!manifest.name) {
    throw new PnpmError('PACKAGE_NAME_NOT_FOUND', `Package name is not defined in the ${manifestFileName}.`)
  }
  if (!validateNpmPackageName(manifest.name).validForOldPackages) {
    throw new PnpmError('INVALID_PACKAGE_NAME', `Invalid package name "${manifest.name}".`)
  }
  if (!manifest.version) {
    throw new PnpmError('PACKAGE_VERSION_NOT_FOUND', `Package version is not defined in the ${manifestFileName}.`)
  }
  const publishManifest = await createPublishManifest({
    projectDir: dir,
    modulesDir: path.join(opts.dir, 'node_modules'),
    manifest,
    embedReadme: opts.embedReadme,
    catalogs: opts.catalogs ?? {},
    hooks: opts.hooks,
    skipManifestObfuscation: opts.skipManifestObfuscation,
  })
  // Strip semver build metadata (the `+<build>` segment) from the published version so that
  // the tarball, the manifest packed inside it, and the metadata sent to the registry all agree.
  // libnpmpublish runs `semver.clean()` on `manifest.version` before computing the provenance
  // subject, which removes build metadata. Leaving it in here would mismatch the version embedded
  // in the tarball's package.json and cause the registry to reject the publish with a 422 when
  // verifying the sigstore provenance bundle. See https://github.com/pnpm/pnpm/issues/11518.
  publishManifest.version = stripBuildMetadata(publishManifest.version!)
  let tarballName: string
  let packDestination: string | undefined
  const normalizedName = manifest.name.replace('@', '').replace('/', '-')
  if (opts.out) {
    if (opts.packDestination) {
      throw new PnpmError('INVALID_OPTION', 'Cannot use --pack-destination and --out together')
    }
    const preparedOut = opts.out.replaceAll('%s', normalizedName).replaceAll('%v', publishManifest.version)
    const parsedOut = path.parse(preparedOut)
    packDestination = parsedOut.dir ? parsedOut.dir : opts.packDestination
    tarballName = parsedOut.base
  } else {
    tarballName = `${normalizedName}-${publishManifest.version}.tgz`
    packDestination = opts.packDestination
  }
  const files = await packlist(dir, {
    manifest: publishManifest as Record<string, unknown>,
    workspaceDir: opts.workspaceDir,
  })
  const filesMap = Object.fromEntries(files.map((file) => [`package/${file}`, path.join(dir, file)]))
  // cspell:disable-next-line
  if (opts.workspaceDir != null && dir !== opts.workspaceDir && !files.some((file) => /LICEN[CS]E(?:\..+)?/i.test(file))) {
    const { workspaceDir } = opts
    const licenses = await glob([LICENSE_GLOB], { cwd: workspaceDir, expandDirectories: false })
    await Promise.all(licenses.map(async (license) => {
      const licensePath = path.join(workspaceDir, license)
      // Only inject a regular file. A symlink could point outside the workspace and leak its
      // target's bytes into the published tarball, so `lstat()` (which does not follow symlinks)
      // rejects it — matching pacquet's inject_workspace_license.
      const stats = await fs.promises.lstat(licensePath)
      if (stats.isFile()) {
        filesMap[`package/${license}`] = licensePath
      }
    }))
  }
  // In `registry` changelog storage the package carries no committed
  // CHANGELOG.md; its section was parked at `pnpm version -r` time and is
  // composed here on top of the previously published version's changelog and
  // packed in. A composed entry supersedes any stale committed CHANGELOG.md.
  const injectedEntries: Record<string, string> = {}
  const composedChangelog = await composeRegistryChangelog(opts, manifest.name, manifest.version)
  if (composedChangelog != null) {
    delete filesMap['package/CHANGELOG.md']
    injectedEntries['package/CHANGELOG.md'] = composedChangelog
  }
  const destDir = packDestination
    ? (path.isAbsolute(packDestination) ? packDestination : path.join(dir, packDestination ?? '.'))
    : dir
  if (!opts.dryRun) {
    await fs.promises.mkdir(destDir, { recursive: true })
  }
  // Derive `contents` and `unpackedSize` from `filesMap` (the full set of tar entries) rather than
  // from `files` (the packlist subset) so that:
  //   - workspace LICENSE files appended to `filesMap` after the packlist call are included; and
  //   - `package.yaml` / `package.json5` entries are reported under the name they actually have in
  //     the tar (`package.json`), since `packPkg()` rewrites them.
  // The `stat()` pass must run before `postpack`, which may delete prepack-generated files that
  // were packed. See https://github.com/pnpm/pnpm/issues/12775.
  const sizes = await Promise.all(Object.entries(filesMap).map(async ([name, source]) => {
    if (isManifestEntry(name)) {
      return Buffer.byteLength(JSON.stringify(publishManifest, null, 2))
    }
    const stat = await fs.promises.stat(source)
    return stat.size
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
  if (!opts.dryRun) {
    await packPkg({
      destFile: path.join(destDir, tarballName),
      filesMap,
      injectedEntries,
      modulesDir: path.join(opts.dir, 'node_modules'),
      packGzipLevel: opts.packGzipLevel,
      manifest: publishManifest,
      bins: [
        ...(await getBinsFromPackageManifest(publishManifest as DependencyManifest, dir)).map(({ path }) => path),
        ...(manifest.publishConfig?.executableFiles ?? [])
          .map((executableFile) => path.join(dir, executableFile)),
      ],
    })
    if (!opts.ignoreScripts) {
      await _runScriptsIfPresent(['postpack'], entryManifest)
    }
  }
  let packedTarballPath
  if (opts.dir !== destDir) {
    packedTarballPath = path.join(destDir, tarballName)
  } else {
    packedTarballPath = path.relative(opts.dir, path.join(dir, tarballName))
  }
  return {
    publishedManifest: await withRegistryReadme(publishManifest, dir),
    packedManifest: publishManifest,
    contents: packedContents,
    tarballPath: packedTarballPath,
    unpackedSize,
  }
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

export interface PackResult {
  /** The manifest sent to the registry as package metadata. See {@link withRegistryReadme}. */
  publishedManifest: ExportedManifest
  /** The `package.json` written into the tarball. */
  packedManifest: ExportedManifest
  contents: string[]
  tarballPath: string
  /** Total uncompressed size of all files in the tarball, in bytes. */
  unpackedSize: number
}

// True when a `package/<path>` tar key names the package manifest, which is
// packed as a single serialized `package/package.json` entry and reported as
// `package.json` in the contents listing regardless of the source file name.
function isManifestEntry (name: string): boolean {
  return name === 'package/package.json' || name === 'package/package.json5' || name === 'package/package.yaml'
}

/**
 * The CHANGELOG.md to pack for a `registry`-storage release: its parked
 * section (written at `pnpm version -r` time) rendered on top of the
 * previously published version's changelog. `undefined` when storage is
 * `repository`, there is no workspace, or the release has no parked section
 * (an ordinary `pnpm pack` of a package that is not mid-release).
 */
async function composeRegistryChangelog (opts: PackOptions, pkgName: string, version: string): Promise<string | undefined> {
  if (changelogStorage(opts.versioning) !== 'registry' || opts.workspaceDir == null) return undefined
  const section = await readPendingChangelog(opts.workspaceDir, pkgName, version)
  if (section == null) return undefined
  const previous = opts.registries != null
    ? await fetchPreviousChangelog(opts as PreviousChangelogOptions, pkgName, version)
    : undefined
  return renderChangelog(previous ?? null, pkgName, section)
}

function stripBuildMetadata (version: string): string {
  const plusIndex = version.indexOf('+')
  return plusIndex === -1 ? version : version.slice(0, plusIndex)
}

function preventBundledDependenciesWithoutHoistedNodeLinker (nodeLinker: Config['nodeLinker'], manifest: ProjectManifest): void {
  if (nodeLinker === 'hoisted') return
  for (const key of ['bundledDependencies', 'bundleDependencies'] as const) {
    const bundledDependencies = manifest[key]
    if (bundledDependencies) {
      throw new PnpmError('BUNDLED_DEPENDENCIES_WITHOUT_HOISTED', `${key} does not work with "nodeLinker: ${nodeLinker}"`, {
        hint: `Add "nodeLinker: hoisted" to pnpm-workspace.yaml or delete ${key} from the root package.json to resolve this error`,
      })
    }
  }
}

async function packPkg (opts: {
  destFile: string
  filesMap: Record<string, string>
  /** In-memory tar entries (name → contents) with no file on disk, e.g. the composed CHANGELOG.md. */
  injectedEntries?: Record<string, string>
  modulesDir: string
  packGzipLevel?: number
  bins: string[]
  manifest: ExportedManifest
}): Promise<void> {
  const {
    destFile,
    filesMap,
    injectedEntries,
    bins,
    manifest,
  } = opts
  const mtime = new Date('1985-10-26T08:15:00.000Z')
  const pack = tar.pack()
  await Promise.all(Object.entries(filesMap).map(async ([name, source]) => {
    const isExecutable = bins.some((bin) => path.relative(bin, source) === '')
    const mode = isExecutable ? 0o755 : 0o644
    if (isManifestEntry(name)) {
      pack.entry({ mode, mtime, name: 'package/package.json' }, JSON.stringify(manifest, null, 2))
      return
    }
    pack.entry({ mode, mtime, name }, fs.readFileSync(source))
  }))
  for (const [name, content] of Object.entries(injectedEntries ?? {})) {
    pack.entry({ mode: 0o644, mtime, name }, content)
  }
  const tarball = fs.createWriteStream(destFile)
  pack.pipe(createGzip({ level: opts.packGzipLevel })).pipe(tarball)
  pack.finalize()
  return new Promise((resolve, reject) => {
    tarball.on('close', () => {
      resolve()
    }).on('error', reject)
  })
}

async function createPublishManifest (opts: {
  projectDir: string
  embedReadme?: boolean
  modulesDir: string
  manifest: ProjectManifest
  catalogs: Catalogs
  hooks?: Hooks
  skipManifestObfuscation?: boolean
}): Promise<ExportedManifest> {
  const { projectDir, embedReadme, modulesDir, manifest, catalogs, hooks, skipManifestObfuscation } = opts
  return createExportableManifest(projectDir, manifest, {
    catalogs,
    hooks,
    embedReadme,
    modulesDir,
    skipManifestObfuscation,
  })
}

function toPackResultJson (packResult: PackResult): PackResultJson {
  const { packedManifest, contents, tarballPath } = packResult
  return {
    name: packedManifest.name as string,
    version: packedManifest.version as string,
    filename: tarballPath,
    files: contents.map((file) => ({ path: file })),
    manifest: packedManifest,
  }
}
