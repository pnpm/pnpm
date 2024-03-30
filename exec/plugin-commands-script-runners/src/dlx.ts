import fs from 'fs'
import path from 'path'
import util from 'util'
import { docsUrl } from '@pnpm/cli-utils'
import { OUTPUT_OPTIONS } from '@pnpm/common-cli-options-help'
import { type Config, types } from '@pnpm/config'
import { createBase32Hash } from '@pnpm/crypto.base32-hash'
import { PnpmError } from '@pnpm/error'
import { add } from '@pnpm/plugin-commands-installation'
import { readPackageJsonFromDir } from '@pnpm/read-package-json'
import { getBinsFromPackageManifest } from '@pnpm/package-bins'
import { getStorePath } from '@pnpm/store-path'
import execa from 'execa'
import omit from 'ramda/src/omit'
import pick from 'ramda/src/pick'
import renderHelp from 'render-help'
import { makeEnv } from './makeEnv'

export const commandNames = ['dlx']

export const shorthands = {
  c: '--shell-mode',
}

export function rcOptionsTypes () {
  return {
    ...pick([
      'use-node-version',
    ], types),
    'shell-mode': Boolean,
  }
}

export const cliOptionsTypes = () => ({
  ...rcOptionsTypes(),
  package: [String, Array],
})

export function help () {
  return renderHelp({
    description: 'Run a package in a temporary environment.',
    descriptionLists: [
      {
        title: 'Options',
        list: [
          {
            description: 'The package to install before running the command',
            name: '--package',
          },
          {
            description: 'Runs the script inside of a shell. Uses /bin/sh on UNIX and \\cmd.exe on Windows.',
            name: '--shell-mode',
            shortAlias: '-c',
          },
        ],
      },
      OUTPUT_OPTIONS,
    ],
    url: docsUrl('dlx'),
    usages: ['pnpm dlx <command> [args...]'],
  })
}

export type DlxCommandOptions = {
  package?: string[]
  shellMode?: boolean
} & Pick<Config, 'reporter' | 'userAgent' | 'cacheDir' | 'dlxCacheMaxAge' > & add.AddCommandOptions

export async function handler (
  opts: DlxCommandOptions,
  [command, ...args]: string[]
) {
  const pkgs = opts.package ?? [command]
  const now = new Date()
  const { storeDir, dlxCacheDir, cacheName } = await getInfo({
    dir: opts.dir,
    pnpmHomeDir: opts.pnpmHomeDir,
    storeDir: opts.storeDir,
    cacheDir: opts.cacheDir,
    pkgs,
  })
  fs.mkdirSync(dlxCacheDir, { recursive: true })
  const { shouldInstall, prefix } = getPrefixInfo({
    dlxCacheMaxAge: opts.dlxCacheMaxAge,
    pid: process.pid,
    cacheName,
    dlxCacheDir,
    now,
  })
  const modulesDir = path.join(prefix, 'node_modules')
  const binsDir = path.join(modulesDir, '.bin')
  const env = makeEnv({ userAgent: opts.userAgent, prependPaths: [binsDir] })
  if (shouldInstall) {
    fs.mkdirSync(prefix, { recursive: true })
    await add.handler({
      // Ideally the config reader should ignore these settings when the dlx command is executed.
      // This is a temporary solution until "@pnpm/config" is refactored.
      ...omit(['workspaceDir', 'rootProjectManifest'], opts),
      bin: binsDir,
      dir: prefix,
      lockfileDir: prefix,
      rootProjectManifestDir: prefix, // This property won't be used as rootProjectManifest will be undefined
      storeDir,
      saveProd: true, // dlx will be looking for the package in the "dependencies" field!
      saveDev: false,
      saveOptional: false,
      savePeer: false,
    }, pkgs)
  }
  const binName = opts.package
    ? command
    : await getBinName(modulesDir, await getPkgName(prefix))
  try {
    await execa(binName, args, {
      cwd: process.cwd(),
      env,
      stdio: 'inherit',
      shell: opts.shellMode ?? false,
    })
  } catch (err: unknown) {
    if (util.types.isNativeError(err) && 'exitCode' in err && err.exitCode != null) {
      return {
        exitCode: err.exitCode,
      }
    }
    throw err
  }
  return { exitCode: 0 }
}

async function getPkgName (pkgDir: string) {
  const manifest = await readPackageJsonFromDir(pkgDir)
  const dependencyNames = Object.keys(manifest.dependencies ?? {})
  if (dependencyNames.length === 0) {
    throw new PnpmError('DLX_NO_DEP', 'dlx was unable to find the installed dependency in "dependencies"')
  }
  return dependencyNames[0]
}

async function getBinName (modulesDir: string, pkgName: string): Promise<string> {
  const pkgDir = path.join(modulesDir, pkgName)
  const manifest = await readPackageJsonFromDir(pkgDir)
  const bins = await getBinsFromPackageManifest(manifest, pkgDir)
  if (bins.length === 0) {
    throw new PnpmError('DLX_NO_BIN', `No binaries found in ${pkgName}`)
  }
  if (bins.length === 1) {
    return bins[0].name
  }
  const scopelessPkgName = scopeless(manifest.name)
  const defaultBin = bins.find(({ name }) => name === scopelessPkgName)
  if (defaultBin) return defaultBin.name
  const binNames = bins.map(({ name }) => name)
  throw new PnpmError('DLX_MULTIPLE_BINS', `Could not determine executable to run. ${pkgName} has multiple binaries: ${binNames.join(', ')}`, {
    hint: `Try one of the following:
${binNames.map(name => `pnpm --package=${pkgName} dlx ${name}`).join('\n')}
`,
  })
}

function scopeless (pkgName: string) {
  if (pkgName[0] === '@') {
    return pkgName.split('/')[1]
  }
  return pkgName
}

async function getInfo (opts: {
  dir: string
  storeDir?: string
  cacheDir: string
  pnpmHomeDir: string
  pkgs: string[]
}) {
  const storeDir = await getStorePath({
    pkgRoot: opts.dir,
    storePath: opts.storeDir,
    pnpmHomeDir: opts.pnpmHomeDir,
  })
  const dlxCacheDir = path.resolve(opts.cacheDir, 'dlx')
  const hashStr = opts.pkgs.join('\n') // '\n' is not a URL-friendly character, and therefore not a valid package name, which can be used as separator
  const cacheName = createBase32Hash(hashStr)
  return { storeDir, dlxCacheDir, cacheName }
}

function getPrefixInfo (opts: {
  dlxCacheDir: string
  cacheName: string
  dlxCacheMaxAge: number
  now: Date
  pid: number
}) {
  const { dlxCacheDir, cacheName, dlxCacheMaxAge, now, pid } = opts
  const cachedPrefix = getCachedPrefix({ dlxCacheDir, cacheName, dlxCacheMaxAge, now })
  const shouldInstall = !cachedPrefix
  const prefix = cachedPrefix ?? getNewPrefix({ dlxCacheDir, cacheName, now, pid })
  return { prefix, shouldInstall }
}

function getCachedPrefix (opts: {
  dlxCacheDir: string
  cacheName: string
  dlxCacheMaxAge: number
  now: Date
}): string | undefined {
  const { dlxCacheDir, cacheName, dlxCacheMaxAge, now } = opts
  return fs.readdirSync(dlxCacheDir, 'utf-8')
    .filter(name => name.startsWith(`${cacheName}-`))
    .map(name => path.join(dlxCacheDir, name))
    .filter(dirPath => isUpToDate(fs.lstatSync(dirPath), dlxCacheMaxAge, now))
    .filter(isFullyInstalled)
    .sort() // directory created at a later date should have greater "date" segment, so it would naturally comes last
    .at(-1) // get the most up-to-date path
}

function isUpToDate (stats: fs.Stats, dlxCacheMaxAge: number, now: Date): boolean {
  return stats.mtime.getTime() + dlxCacheMaxAge * 60_000 >= now.getTime()
}

function isFullyInstalled (dirPath: string): boolean {
  return fs.existsSync(path.join(dirPath, 'node_modules', '.modules.yaml'))
}

function getNewPrefix (opts: {
  dlxCacheDir: string
  cacheName: string
  now: Date
  pid: number
}): string {
  const { dlxCacheDir, cacheName, now, pid } = opts
  const name = `${cacheName}-${now.getTime().toString(16)}-${pid.toString(16)}`
  return path.join(dlxCacheDir, name)
}
