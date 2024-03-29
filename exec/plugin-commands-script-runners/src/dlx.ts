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
  const { storeDir, contentDir, linkName } = await getInfo({
    pid: process.pid,
    dir: opts.dir,
    pnpmHomeDir: opts.pnpmHomeDir,
    storeDir: opts.storeDir,
    cacheDir: opts.cacheDir,
    pkgs,
    now,
  })
  let prefix: string
  let shouldInstall: boolean
  let shouldRenewLink: boolean
  let shouldDeleteContent: boolean
  const { linkAlreadyExists, cacheFullyInstalled, cacheUpToDate } = acquireCacheLock({
    dlxCacheMaxAge: opts.dlxCacheMaxAge,
    contentDir,
    linkName,
    now,
  })
  if (cacheUpToDate) {
    prefix = linkName
    shouldInstall = false
    shouldRenewLink = false
    shouldDeleteContent = false
  } else if (cacheFullyInstalled) {
    prefix = contentDir
    shouldInstall = true
    fs.mkdirSync(contentDir, { recursive: true })
    shouldRenewLink = true
    shouldDeleteContent = false
  } else if (linkAlreadyExists) {
    prefix = contentDir
    shouldInstall = true
    fs.mkdirSync(contentDir, { recursive: true })
    shouldRenewLink = false
    shouldDeleteContent = true
  } else {
    prefix = linkName
    shouldInstall = true
    shouldRenewLink = true
    shouldDeleteContent = false
  }
  const modulesDir = path.join(prefix, 'node_modules')
  const binsDir = path.join(modulesDir, '.bin')
  const env = makeEnv({ userAgent: opts.userAgent, prependPaths: [binsDir] })
  if (shouldInstall) {
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
  } finally {
    if (shouldRenewLink) {
      try {
        fs.unlinkSync(linkName)
        fs.symlinkSync(contentDir, linkName, 'junction')
      } catch { }
    }

    if (shouldDeleteContent) {
      try {
        fs.rmSync(contentDir, { recursive: true })
      } catch { }
    }
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
  now: Date
  pid: number
}) {
  const storeDir = await getStorePath({
    pkgRoot: opts.dir,
    storePath: opts.storeDir,
    pnpmHomeDir: opts.pnpmHomeDir,
  })
  const cacheDir = path.resolve(opts.cacheDir, 'dlx')
  const cacheInfo = getCacheInfo({
    cacheDir,
    now: opts.now,
    pkgs: opts.pkgs,
    pid: opts.pid,
  })
  return {
    storeDir,
    cacheDir,
    ...cacheInfo,
  }
}

function getCacheInfo (opts: {
  cacheDir: string
  pkgs: string[]
  now: Date
  pid: number
}) {
  const { cacheDir, pkgs, now, pid } = opts
  const hashStr = pkgs.join('\n') // '\n' is not a URL-friendly character, and therefore not a valid package name, which can be used as separator
  const cacheName = createBase32Hash(hashStr)
  const linkName = path.join(cacheDir, cacheName)
  const contentDir = getContentDir({ cacheDir, cacheName, now, pid })
  return { cacheName, linkName, contentDir }
}

function getContentDir (opts: {
  cacheDir: string
  cacheName: string
  now: Date
  pid: number
}) {
  const { cacheDir, cacheName, now, pid } = opts
  const name = `${cacheName}-${now.getTime().toString(16)}-${pid.toString(16)}`
  return path.join(cacheDir, name)
}

function acquireCacheLock (opts: {
  linkName: string
  contentDir: string
  now: Date
  dlxCacheMaxAge: number
}) {
  const { linkName, contentDir, now, dlxCacheMaxAge } = opts
  fs.mkdirSync(path.dirname(linkName), { recursive: true })
  let linkAlreadyExists: boolean
  try {
    fs.symlinkSync(contentDir, linkName, 'junction')
    linkAlreadyExists = false
  } catch (err) {
    if (util.types.isNativeError(err) && 'code' in err && err.code === 'EEXIST') {
      linkAlreadyExists = true
    } else {
      throw err
    }
  }
  const cacheFullyInstalled = linkAlreadyExists && fs.existsSync(path.join(linkName, 'node_modules', '.modules.yaml'))
  const cacheUpToDate = cacheFullyInstalled && fs.statSync(linkName).mtime.getTime() + dlxCacheMaxAge * 60_000 <= now.getTime()
  return { linkAlreadyExists, cacheFullyInstalled, cacheUpToDate }
}
