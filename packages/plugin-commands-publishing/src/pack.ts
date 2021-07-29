import fs from 'fs'
import path from 'path'
import { createGzip } from 'zlib'
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
  opts: Pick<UniversalOptions, 'dir'> & Pick<Config, 'ignoreScripts' | 'rawConfig'> & Partial<Pick<Config, 'extraBinPaths'>> & {
    argv: {
      original: string[]
    }
    engineStrict?: boolean
    packDestination?: string
    workspaceDir?: string
  }
) {
  const { manifest: entryManifest } = await readProjectManifest(opts.dir, opts)
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
  const tarballName = `${manifest.name!.replace('@', '').replace('/', '-')}-${manifest.version!}.tgz`
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
  await packPkg(path.join(destDir, tarballName), filesMap, dir)
  if (!opts.ignoreScripts) {
    await _runScriptsIfPresent(['postpack'], entryManifest)
  }
  return path.relative(opts.dir, path.join(dir, tarballName))
}

async function packPkg (destFile: string, filesMap: Record<string, string>, projectDir: string): Promise<void> {
  const { manifest } = await readProjectManifest(projectDir, {})
  const bins = await binify(manifest as DependencyManifest, projectDir)
  const mtime = new Date('1985-10-26T08:15:00.000Z')
  const pack = tar.pack()
  for (const [name, source] of Object.entries(filesMap)) {
    const isExecutable = bins.some((bin) => path.relative(bin.path, source) === '')
    const mode = isExecutable ? 0o755 : 0o644
    if (/^package\/package\.(json|json5|yaml)/.test(name)) {
      const publishManifest = await exportableManifest(projectDir, manifest)
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
