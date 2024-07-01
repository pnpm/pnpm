import fs, { type Stats } from 'fs'
import path from 'path'
import util from 'util'
import { docsUrl } from '@pnpm/cli-utils'
import { OUTPUT_OPTIONS } from '@pnpm/common-cli-options-help'
import { type Config, types } from '@pnpm/config'
import { createBase32Hash } from '@pnpm/crypto.base32-hash'
import { PnpmError } from '@pnpm/error'
import { node } from '@pnpm/plugin-commands-env'
import { add } from '@pnpm/plugin-commands-installation'
import { readPackageJsonFromDir } from '@pnpm/read-package-json'
import { getBinsFromPackageManifest } from '@pnpm/package-bins'
import execa from 'execa'
import omit from 'ramda/src/omit'
import pick from 'ramda/src/pick'
import renderHelp from 'render-help'
import symlinkDir from 'symlink-dir'
import { makeEnv } from './makeEnv'

export const commandNames = ['dlx']

export const shorthands: Record<string, string> = {
  c: '--shell-mode',
}

export function rcOptionsTypes (): Record<string, unknown> {
  return {
    ...pick([
      'use-node-version',
    ], types),
    'shell-mode': Boolean,
  }
}

export const cliOptionsTypes = (): Record<string, unknown> => ({
  ...rcOptionsTypes(),
  package: [String, Array],
})

export function help (): string {
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
} & Pick<Config, 'reporter' | 'userAgent' | 'cacheDir' | 'dlxCacheMaxAge' | 'useNodeVersion' > & add.AddCommandOptions

export async function handler (
  opts: DlxCommandOptions,
  [command, ...args]: string[]
): Promise<{ exitCode: number }> {
  const pkgs = opts.package ?? [command]
  const { cacheLink, prepareDir } = findCache(pkgs, {
    dlxCacheMaxAge: opts.dlxCacheMaxAge,
    cacheDir: opts.cacheDir,
  })
  if (prepareDir) {
    fs.mkdirSync(prepareDir, { recursive: true })
    await add.handler({
      // Ideally the config reader should ignore these settings when the dlx command is executed.
      // This is a temporary solution until "@pnpm/config" is refactored.
      ...omit(['workspaceDir', 'rootProjectManifest'], opts),
      bin: path.join(prepareDir, 'node_modules/.bin'),
      dir: prepareDir,
      lockfileDir: prepareDir,
      rootProjectManifestDir: prepareDir, // This property won't be used as rootProjectManifest will be undefined
      saveProd: true, // dlx will be looking for the package in the "dependencies" field!
      saveDev: false,
      saveOptional: false,
      savePeer: false,
    }, pkgs)
    await symlinkDir(prepareDir, cacheLink, { overwrite: true })
  }
  const modulesDir = path.join(cacheLink, 'node_modules')
  const binsDir = path.join(modulesDir, '.bin')
  const prependPaths = [binsDir]
  if (opts.useNodeVersion) {
    const nodePath = await node.getNodeBinDir(opts)
    prependPaths.push(nodePath)
  }
  const env = makeEnv({ userAgent: opts.userAgent, prependPaths })
  const binName = opts.package
    ? command
    : await getBinName(modulesDir, await getPkgName(cacheLink))
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
        exitCode: err.exitCode as number,
      }
    }
    throw err
  }
  return { exitCode: 0 }
}

async function getPkgName (pkgDir: string): Promise<string> {
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

function scopeless (pkgName: string): string {
  if (pkgName[0] === '@') {
    return pkgName.split('/')[1]
  }
  return pkgName
}

function findCache (pkgs: string[], opts: {
  cacheDir: string
  dlxCacheMaxAge: number
}): { cacheLink: string, prepareDir: string | null } {
  const dlxCommandCacheDir = createDlxCommandCacheDir(opts.cacheDir, pkgs)
  const cacheLink = path.join(dlxCommandCacheDir, 'pkg')
  const valid = isCacheValid(cacheLink, opts.dlxCacheMaxAge)
  const prepareDir = valid ? null : getPrepareDir(dlxCommandCacheDir)
  return { cacheLink, prepareDir }
}

function createDlxCommandCacheDir (cacheDir: string, pkgs: string[]): string {
  const dlxCacheDir = path.resolve(cacheDir, 'dlx')
  const hashStr = pkgs.join('\n') // '\n' is not a URL-friendly character, and therefore not a valid package name, which can be used as separator
  const cacheKey = createBase32Hash(hashStr)
  const cachePath = path.join(dlxCacheDir, cacheKey)
  fs.mkdirSync(cachePath, { recursive: true })
  return cachePath
}

function isCacheValid (cacheLink: string, dlxCacheMaxAge: number): boolean {
  let stats: Stats
  try {
    stats = fs.lstatSync(cacheLink)
  } catch (err) {
    if (util.types.isNativeError(err) && 'code' in err && err.code === 'ENOENT') {
      return false
    }
    throw err
  }
  return stats.mtime.getTime() + dlxCacheMaxAge * 60_000 >= new Date().getTime()
}

function getPrepareDir (cachePath: string): string {
  const name = `${new Date().getTime().toString(16)}-${process.pid.toString(16)}`
  return path.join(cachePath, name)
}
