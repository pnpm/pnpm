import { type Stats } from 'fs'
import fs from 'fs/promises'
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
import symlinkDir from 'symlink-dir'
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
  const { storeDir, cachePath } = await getInfo({
    dir: opts.dir,
    pnpmHomeDir: opts.pnpmHomeDir,
    storeDir: opts.storeDir,
    cacheDir: opts.cacheDir,
    pkgs,
  })
  await fs.mkdir(cachePath, { recursive: true })
  const { cacheLink, newPrefix, prefix } = await getPrefixInfo({
    dlxCacheMaxAge: opts.dlxCacheMaxAge,
    pid: process.pid,
    cachePath,
    now,
  })
  const modulesDir = path.join(prefix, 'node_modules')
  const binsDir = path.join(modulesDir, '.bin')
  const env = makeEnv({ userAgent: opts.userAgent, prependPaths: [binsDir] })
  if (newPrefix) {
    await fs.mkdir(newPrefix, { recursive: true })
    await add.handler({
      // Ideally the config reader should ignore these settings when the dlx command is executed.
      // This is a temporary solution until "@pnpm/config" is refactored.
      ...omit(['workspaceDir', 'rootProjectManifest'], opts),
      bin: binsDir,
      dir: newPrefix,
      lockfileDir: newPrefix,
      rootProjectManifestDir: newPrefix, // This property won't be used as rootProjectManifest will be undefined
      storeDir,
      saveProd: true, // dlx will be looking for the package in the "dependencies" field!
      saveDev: false,
      saveOptional: false,
      savePeer: false,
    }, pkgs)
    await symlinkDir(newPrefix, cacheLink, { overwrite: true })
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
  const cachePath = path.join(dlxCacheDir, cacheName)
  return { storeDir, dlxCacheDir, cacheName, cachePath }
}

async function getPrefixInfo (opts: {
  cachePath: string
  dlxCacheMaxAge: number
  now: Date
  pid: number
}) {
  const { cachePath, dlxCacheMaxAge, now, pid } = opts
  const cacheLink = path.join(cachePath, 'link')
  const cacheStatus = await checkCacheLink(cacheLink, dlxCacheMaxAge, now)
  const shouldInstall = cacheStatus === 'not-exist' || cacheStatus === 'out-of-date'
  const newPrefix = shouldInstall ? getNewPrefix(cachePath, now, pid) : null
  const prefix = newPrefix ?? cacheLink
  return { cacheLink, cacheStatus, shouldInstall, newPrefix, prefix }
}

async function checkCacheLink (cacheLink: string, dlxCacheMaxAge: number, now: Date): Promise<'not-exist' | 'out-of-date' | 'up-to-date'> {
  let stats: Stats
  try {
    stats = await fs.lstat(cacheLink)
  } catch (err) {
    if (util.types.isNativeError(err) && 'code' in err && err.code === 'ENOENT') {
      return 'not-exist'
    }
    throw err
  }
  if (stats.mtime.getTime() + dlxCacheMaxAge * 60_000 < now.getTime()) {
    return 'out-of-date'
  }
  return 'up-to-date'
}

function getNewPrefix (cachePath: string, now: Date, pid: number): string {
  const name = `${now.getTime().toString(16)}-${pid.toString(16)}`
  return path.join(cachePath, name)
}
