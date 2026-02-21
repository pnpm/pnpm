import fs from 'fs'
import path from 'path'
import { createGzip } from 'zlib'
import { type Catalogs } from '@pnpm/catalogs.types'
import { PnpmError } from '@pnpm/error'
import { types as allTypes, type UniversalOptions, type Config, getWorkspaceConcurrency, getDefaultWorkspaceConcurrency } from '@pnpm/config'
import { readProjectManifest } from '@pnpm/cli-utils'
import { type ExportedManifest, createExportableManifest } from '@pnpm/exportable-manifest'
import { packlist } from '@pnpm/fs.packlist'
import { getBinsFromPackageManifest } from '@pnpm/package-bins'
import { type Hooks } from '@pnpm/pnpmfile'
import { type ProjectManifest, type Project, type ProjectRootDir, type ProjectsGraph, type DependencyManifest } from '@pnpm/types'
import { glob } from 'tinyglobby'
import { pick } from 'ramda'
import realpathMissing from 'realpath-missing'
import renderHelp from 'render-help'
import tar from 'tar-stream'
import { runScriptsIfPresent } from './publish.js'
import chalk from 'chalk'
import validateNpmPackageName from 'validate-npm-package-name'
import pLimit from 'p-limit'
import { FILTERING } from '@pnpm/common-cli-options-help'
import { sortPackages } from '@pnpm/sort-packages'
import { logger } from '@pnpm/logger'

const LICENSE_GLOB = 'LICEN{S,C}E{,.*}' // cspell:disable-line

export function rcOptionsTypes (): Record<string, unknown> {
  return {
    ...cliOptionsTypes(),
    ...pick([
      'npm-path',
    ], allTypes),
  }
}

export function cliOptionsTypes (): Record<string, unknown> {
  return {
    out: String,
    recursive: Boolean,
    ...pick([
      'dry-run',
      'pack-destination',
      'pack-gzip-level',
      'json',
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
            description: 'Prints the packed tarball and contents in the json format.',
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
| 'rawConfig'
| 'embedReadme'
| 'packGzipLevel'
| 'nodeLinker'
> & Partial<Pick<Config, 'extraBinPaths'
| 'extraEnv'
| 'hooks'
| 'recursive'
| 'selectedProjectsGraph'
| 'workspaceConcurrency'
| 'workspaceDir'
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

    const chunks = sortPackages(selectedProjectsGraph)

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
    rawConfig: opts.rawConfig,
    rootModulesDir: await realpathMissing(path.join(opts.dir, 'node_modules')),
    stdio: 'inherit',
    unsafePerm: true, // when running scripts explicitly, assume that they're trusted.
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
  let tarballName: string
  let packDestination: string | undefined
  const normalizedName = manifest.name.replace('@', '').replace('/', '-')
  if (opts.out) {
    if (opts.packDestination) {
      throw new PnpmError('INVALID_OPTION', 'Cannot use --pack-destination and --out together')
    }
    const preparedOut = opts.out.replaceAll('%s', normalizedName).replaceAll('%v', manifest.version)
    const parsedOut = path.parse(preparedOut)
    packDestination = parsedOut.dir ? parsedOut.dir : opts.packDestination
    tarballName = parsedOut.base
  } else {
    tarballName = `${normalizedName}-${manifest.version}.tgz`
    packDestination = opts.packDestination
  }
  const publishManifest = await createPublishManifest({
    projectDir: dir,
    modulesDir: path.join(opts.dir, 'node_modules'),
    manifest,
    embedReadme: opts.embedReadme,
    catalogs: opts.catalogs ?? {},
    hooks: opts.hooks,
  })
  const files = await packlist(dir, {
    manifest: publishManifest as Record<string, unknown>,
  })
  const filesMap = Object.fromEntries(files.map((file) => [`package/${file}`, path.join(dir, file)]))
  // cspell:disable-next-line
  if (opts.workspaceDir != null && dir !== opts.workspaceDir && !files.some((file) => /LICEN[CS]E(?:\..+)?/i.test(file))) {
    const licenses = await glob([LICENSE_GLOB], { cwd: opts.workspaceDir, expandDirectories: false })
    for (const license of licenses) {
      filesMap[`package/${license}`] = path.join(opts.workspaceDir, license)
    }
  }
  const destDir = packDestination
    ? (path.isAbsolute(packDestination) ? packDestination : path.join(dir, packDestination ?? '.'))
    : dir
  if (!opts.dryRun) {
    await fs.promises.mkdir(destDir, { recursive: true })
    await packPkg({
      destFile: path.join(destDir, tarballName),
      filesMap,
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
  const packedContents = files.sort((a, b) => a.localeCompare(b, 'en'))
  return {
    publishedManifest: publishManifest,
    contents: packedContents,
    tarballPath: packedTarballPath,
  }
}

export interface PackResult {
  publishedManifest: ExportedManifest
  contents: string[]
  tarballPath: string
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

async function readReadmeFile (projectDir: string): Promise<string | undefined> {
  const files = await fs.promises.readdir(projectDir)
  const readmePath = files.find(name => /readme\.md$/i.test(name))
  const readmeFile = readmePath ? await fs.promises.readFile(path.join(projectDir, readmePath), 'utf8') : undefined

  return readmeFile
}

async function packPkg (opts: {
  destFile: string
  filesMap: Record<string, string>
  modulesDir: string
  packGzipLevel?: number
  bins: string[]
  manifest: ExportedManifest
}): Promise<void> {
  const {
    destFile,
    filesMap,
    bins,
    manifest,
  } = opts
  const mtime = new Date('1985-10-26T08:15:00.000Z')
  const pack = tar.pack()
  await Promise.all(Object.entries(filesMap).map(async ([name, source]) => {
    const isExecutable = bins.some((bin) => path.relative(bin, source) === '')
    const mode = isExecutable ? 0o755 : 0o644
    if (/^package\/package\.(?:json|json5|yaml)$/.test(name)) {
      pack.entry({ mode, mtime, name: 'package/package.json' }, JSON.stringify(manifest, null, 2))
      return
    }
    pack.entry({ mode, mtime, name }, fs.readFileSync(source))
  }))
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
}): Promise<ExportedManifest> {
  const { projectDir, embedReadme, modulesDir, manifest, catalogs, hooks } = opts
  const readmeFile = embedReadme ? await readReadmeFile(projectDir) : undefined
  return createExportableManifest(projectDir, manifest, {
    catalogs,
    hooks,
    readmeFile,
    modulesDir,
  })
}

function toPackResultJson (packResult: PackResult): PackResultJson {
  const { publishedManifest, contents, tarballPath } = packResult
  return {
    name: publishedManifest.name as string,
    version: publishedManifest.version as string,
    filename: tarballPath,
    files: contents.map((file) => ({ path: file })),
  }
}
