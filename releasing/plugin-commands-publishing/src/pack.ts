import fs from 'fs'
import path from 'path'
import { createGzip } from 'zlib'
import { type Catalogs } from '@pnpm/catalogs.types'
import { PnpmError } from '@pnpm/error'
import { types as allTypes, type UniversalOptions, type Config } from '@pnpm/config'
import { readProjectManifest } from '@pnpm/cli-utils'
import { createExportableManifest } from '@pnpm/exportable-manifest'
import { packlist } from '@pnpm/fs.packlist'
import { getBinsFromPackageManifest } from '@pnpm/package-bins'
import { type ProjectManifest, type DependencyManifest } from '@pnpm/types'
import fg from 'fast-glob'
import pick from 'ramda/src/pick'
import realpathMissing from 'realpath-missing'
import renderHelp from 'render-help'
import tar from 'tar-stream'
import { runScriptsIfPresent } from './publish'

const LICENSE_GLOB = 'LICEN{S,C}E{,.*}' // cspell:disable-line
const findLicenses = fg.bind(fg, [LICENSE_GLOB]) as (opts: { cwd: string }) => Promise<string[]>

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
    'pack-destination': String,
    ...pick([
      'pack-gzip-level',
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
            description: 'Directory in which `pnpm pack` will save tarballs. The default is the current working directory.',
            name: '--pack-destination <dir>',
          },
        ],
      },
    ],
  })
}

export async function handler (
  opts: Pick<UniversalOptions, 'dir'> & Pick<Config, 'catalogs' | 'ignoreScripts' | 'rawConfig' | 'embedReadme' | 'packGzipLevel' | 'nodeLinker'> & Partial<Pick<Config, 'extraBinPaths' | 'extraEnv'>> & {
    argv: {
      original: string[]
    }
    engineStrict?: boolean
    packDestination?: string
    workspaceDir?: string
  }
): Promise<string> {
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
  if (!manifest.version) {
    throw new PnpmError('PACKAGE_VERSION_NOT_FOUND', `Package version is not defined in the ${manifestFileName}.`)
  }
  const tarballName = `${manifest.name.replace('@', '').replace('/', '-')}-${manifest.version}.tgz`
  const publishManifest = await createPublishManifest({
    projectDir: dir,
    modulesDir: path.join(opts.dir, 'node_modules'),
    manifest,
    embedReadme: opts.embedReadme,
    catalogs: opts.catalogs ?? {},
  })
  const files = await packlist(dir, {
    packageJsonCache: {
      [path.join(dir, 'package.json')]: publishManifest as Record<string, unknown>,
    },
  })
  const filesMap = Object.fromEntries(files.map((file) => [`package/${file}`, path.join(dir, file)]))
  // cspell:disable-next-line
  if (opts.workspaceDir != null && dir !== opts.workspaceDir && !files.some((file) => /LICEN[CS]E(\..+)?/i.test(file))) {
    const licenses = await findLicenses({ cwd: opts.workspaceDir })
    for (const license of licenses) {
      filesMap[`package/${license}`] = path.join(opts.workspaceDir, license)
    }
  }
  const destDir = opts.packDestination
    ? (path.isAbsolute(opts.packDestination) ? opts.packDestination : path.join(dir, opts.packDestination ?? '.'))
    : dir
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
  if (opts.dir !== destDir) {
    return path.join(destDir, tarballName)
  }
  return path.relative(opts.dir, path.join(dir, tarballName))
}

function preventBundledDependenciesWithoutHoistedNodeLinker (nodeLinker: Config['nodeLinker'], manifest: ProjectManifest): void {
  if (nodeLinker === 'hoisted') return
  for (const key of ['bundledDependencies', 'bundleDependencies'] as const) {
    const bundledDependencies = manifest[key]
    if (bundledDependencies) {
      throw new PnpmError('BUNDLED_DEPENDENCIES_WITHOUT_HOISTED', `${key} does not work with node-linker=${nodeLinker}`, {
        hint: `Add node-linker=hoisted to .npmrc or delete ${key} from the root package.json to resolve this error`,
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
  manifest: ProjectManifest
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
    if (/^package\/package\.(json|json5|yaml)/.test(name)) {
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
}): Promise<ProjectManifest> {
  const { projectDir, embedReadme, modulesDir, manifest, catalogs } = opts
  const readmeFile = embedReadme ? await readReadmeFile(projectDir) : undefined
  return createExportableManifest(projectDir, manifest, {
    catalogs,
    readmeFile,
    modulesDir,
  })
}
