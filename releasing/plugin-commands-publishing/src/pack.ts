import fs from 'fs'
import path from 'path'
import { createGzip } from 'zlib'
import { PnpmError } from '@pnpm/error'
import { types as allTypes, type UniversalOptions, type Config } from '@pnpm/config'
import { readProjectManifest } from '@pnpm/cli-utils'
import { createExportableManifest } from '@pnpm/exportable-manifest'
import { getBinsFromPackageManifest } from '@pnpm/package-bins'
import { type DependencyManifest } from '@pnpm/types'
import fg from 'fast-glob'
import pick from 'ramda/src/pick'
import realpathMissing from 'realpath-missing'
import renderHelp from 'render-help'
import tar from 'tar-stream'
import packlist from 'npm-packlist'
import { runScriptsIfPresent } from './publish'

const LICENSE_GLOB = 'LICEN{S,C}E{,.*}'
const findLicenses = fg.bind(fg, [LICENSE_GLOB]) as (opts: { cwd: string }) => Promise<string[]>

export function rcOptionsTypes () {
  return {
    ...cliOptionsTypes(),
    ...pick([
      'npm-path',
    ], allTypes),
  }
}

export function cliOptionsTypes () {
  return {
    'pack-destination': String,
    ...pick([
      'pack-gzip-level',
    ], allTypes),
  }
}

export const commandNames = ['pack']

export function help () {
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
  opts: Pick<UniversalOptions, 'dir'> & Pick<Config, 'ignoreScripts' | 'rawConfig' | 'embedReadme' | 'packGzipLevel'> & Partial<Pick<Config, 'extraBinPaths' | 'extraEnv'>> & {
    argv: {
      original: string[]
    }
    engineStrict?: boolean
    packDestination?: string
    workspaceDir?: string
  }
) {
  const { manifest: entryManifest, fileName: manifestFileName } = await readProjectManifest(opts.dir, opts)
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
  const manifest = (opts.dir !== dir) ? (await readProjectManifest(dir, opts)).manifest : entryManifest
  if (!manifest.name) {
    throw new PnpmError('PACKAGE_NAME_NOT_FOUND', `Package name is not defined in the ${manifestFileName}.`)
  }
  if (!manifest.version) {
    throw new PnpmError('PACKAGE_VERSION_NOT_FOUND', `Package version is not defined in the ${manifestFileName}.`)
  }
  const tarballName = `${manifest.name.replace('@', '').replace('/', '-')}-${manifest.version}.tgz`
  const files = await packlist({ path: dir })
  const filesMap: Record<string, string> = Object.fromEntries(files.map((file) => [`package/${file}`, path.join(dir, file)]))
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
    projectDir: dir,
    embedReadme: opts.embedReadme,
    modulesDir: path.join(opts.dir, 'node_modules'),
    packGzipLevel: opts.packGzipLevel,
  })
  if (!opts.ignoreScripts) {
    await _runScriptsIfPresent(['postpack'], entryManifest)
  }
  if (opts.dir !== destDir) {
    return path.join(destDir, tarballName)
  }
  return path.relative(opts.dir, path.join(dir, tarballName))
}

async function readReadmeFile (filesMap: Record<string, string>) {
  const readmePath = Object.keys(filesMap).find(name => /^package\/readme\.md$/i.test(name))
  const readmeFile = readmePath ? await fs.promises.readFile(filesMap[readmePath], 'utf8') : undefined

  return readmeFile
}

async function packPkg (opts: {
  destFile: string
  filesMap: Record<string, string>
  projectDir: string
  embedReadme?: boolean
  modulesDir: string
  packGzipLevel?: number
}): Promise<void> {
  const {
    destFile,
    filesMap,
    projectDir,
    embedReadme,
  } = opts
  const { manifest } = await readProjectManifest(projectDir, {})
  const bins = [
    ...(await getBinsFromPackageManifest(manifest as DependencyManifest, projectDir)).map(({ path }) => path),
    ...(manifest.publishConfig?.executableFiles ?? [])
      .map((executableFile) => path.join(projectDir, executableFile)),
  ]
  const mtime = new Date('1985-10-26T08:15:00.000Z')
  const pack = tar.pack()
  await Promise.all(Object.entries(filesMap).map(async ([name, source]) => {
    const isExecutable = bins.some((bin) => path.relative(bin, source) === '')
    const mode = isExecutable ? 0o755 : 0o644
    if (/^package\/package\.(json|json5|yaml)/.test(name)) {
      const readmeFile = embedReadme ? await readReadmeFile(filesMap) : undefined
      const publishManifest = await createExportableManifest(projectDir, manifest, { readmeFile, modulesDir: opts.modulesDir })
      pack.entry({ mode, mtime, name: 'package/package.json' }, JSON.stringify(publishManifest, null, 2))
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
