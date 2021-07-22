import fs from 'fs'
import path from 'path'
import { types as allTypes, UniversalOptions, Config } from '@pnpm/config'
import { readProjectManifest } from '@pnpm/cli-utils'
import exportableManifest from '@pnpm/exportable-manifest'
import fg from 'fast-glob'
import gunzip from 'gunzip-maybe'
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
  return {}
}

export const commandNames = ['pack']

export function help () {
  return renderHelp({
    description: 'Creates a compressed gzip archive of package dependencies.',
    usages: ['pnpm pack'],
  })
}

export async function handler (
  opts: Pick<UniversalOptions, 'dir'> & Pick<Config, 'ignoreScripts' | 'rawConfig'> & Partial<Pick<Config, 'extraBinPaths'>> & {
    argv: {
      original: string[]
    }
    engineStrict?: boolean
    workspaceDir?: string
  }
) {
  const { manifest } = await readProjectManifest(opts.dir, opts)
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
    ], manifest)
  }
  const tarballName = `${manifest.name!.replace('@', '').replace('/', '-')}-${manifest.version!}.tgz`
  const files = await packlist({ path: opts.dir })
  const filesMap: Record<string, string> = fromPairs(files.map((file) => [`package/${file}`, path.join(opts.dir, file)]))
  if (opts.workspaceDir != null && opts.dir !== opts.workspaceDir && !files.some((file) => /LICEN[CS]E(\..+)?/i.test(file))) {
    const licenses = await findLicenses({ cwd: opts.workspaceDir })
    for (const license of licenses) {
      filesMap[`package/${license}`] = path.join(opts.workspaceDir, license)
    }
  }
  await packPkg(path.join(opts.dir, tarballName), filesMap, opts.dir)
  if (!opts.ignoreScripts) {
    await _runScriptsIfPresent(['postpack'], manifest)
  }
  return tarballName
}

async function packPkg (destFile: string, filesMap: Record<string, string>, projectDir: string): Promise<void> {
  const pack = tar.pack()
  for (const [name, source] of Object.entries(filesMap)) {
    if (/^package\/package\.(json|json5|yaml)/.test(name)) {
      const { manifest } = await readProjectManifest(projectDir, {})
      const publishManifest = await exportableManifest(projectDir, manifest)
      pack.entry({ name: 'package/package.json' }, JSON.stringify(publishManifest, null, 2))
      continue
    }
    pack.entry({ name }, fs.readFileSync(source))
  }
  const tarball = fs.createWriteStream(destFile)
  pack.pipe(gunzip()).pipe(tarball)
  pack.finalize()
  return new Promise((resolve, reject) => {
    tarball.on('close', () => resolve()).on('error', reject)
  })
}
