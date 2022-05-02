import fs from 'fs'
import path from 'path'
import { createGzip } from 'zlib'
import PnpmError from '@pnpm/error'
import { types as allTypes, UniversalOptions, Config } from '@pnpm/config'
import { readProjectManifest } from '@pnpm/cli-utils'
import exportableManifest from '@pnpm/exportable-manifest'
import binify from '@pnpm/package-bins'
import { DependencyManifest } from '@pnpm/types'
import fg from 'fast-glob'
import pick from 'ramda/src/pick'
import realpathMissing from 'realpath-missing'
import renderHelp from 'render-help'
import tar from 'tar-stream'
import packlist from 'npm-packlist'
import fromPairs from 'ramda/src/fromPairs'
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
  opts: Pick<UniversalOptions, 'dir'> & Pick<Config, 'ignoreScripts' | 'rawConfig' | 'embedReadme'> & Partial<Pick<Config, 'extraBinPaths'>> & {
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
    pkgRoot: opts.dir,
    rawConfig: opts.rawConfig,
    rootModulesDir: await realpathMissing(path.join(opts.dir, 'node_modules')),
    stdio: 'inherit',
    unsafePerm: true, // when running scripts explicitly, assume that they're trusted.
  })
  if (!opts.ignoreScripts) {
    await _runScriptsIfPresent([
      'prepublish',
      'prepare',
      'prepublishOnly',
      'prepack',
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
  const filesMap: Record<string, string> = fromPairs(files.map((file) => [`package/${file}`, path.join(dir, file)]))
  if (opts.workspaceDir != null && dir !== opts.workspaceDir && !files.some((file) => /LICEN[CS]E(\..+)?/i.test(file))) {
    const licenses = await findLicenses({ cwd: opts.workspaceDir })
    for (const license of licenses) {
      filesMap[`package/${license}`] = path.join(opts.workspaceDir, license)
    }
  }
  const destDir = opts.packDestination
    ? (path.isAbsolute(opts.packDestination) ? opts.packDestination : path.join(dir, opts.packDestination ?? '.'))
    : dir
  await packPkg({
    destFile: path.join(destDir, tarballName),
    filesMap,
    projectDir: dir,
    embedReadme: opts.embedReadme,
  })
  if (!opts.ignoreScripts) {
    await _runScriptsIfPresent(['postpack'], entryManifest)
  }
  if (opts.dir !== dir) {
    return path.join(dir, tarballName)
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
}): Promise<void> {
  const {
    destFile,
    filesMap,
    projectDir,
    embedReadme,
  } = opts
  const { manifest } = await readProjectManifest(projectDir, {})
  const bins = [
    ...(await binify(manifest as DependencyManifest, projectDir)).map(({ path }) => path),
    ...(manifest.publishConfig?.executableFiles ?? [])
      .map((executableFile) => path.join(projectDir, executableFile)),
  ]
  const mtime = new Date('1985-10-26T08:15:00.000Z')
  const pack = tar.pack()
  for (const [name, source] of Object.entries(filesMap)) {
    const isExecutable = bins.some((bin) => path.relative(bin, source) === '')
    const mode = isExecutable ? 0o755 : 0o644
    if (/^package\/package\.(json|json5|yaml)/.test(name)) {
      const readmeFile = embedReadme ? await readReadmeFile(filesMap) : undefined
      const publishManifest = await exportableManifest(projectDir, manifest, { readmeFile })
      pack.entry({ mode, mtime, name: 'package/package.json' }, JSON.stringify(publishManifest, null, 2))
      continue
    }
    pack.entry({ mode, mtime, name }, fs.readFileSync(source))
  }
  const tarball = fs.createWriteStream(destFile)
  pack.pipe(createGzip()).pipe(tarball)
  pack.finalize()
  return new Promise((resolve, reject) => {
    tarball.on('close', () => resolve()).on('error', reject)
  })
}
