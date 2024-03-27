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
      'dlx-cache-max-age',
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
  const { storeDir, tempDir, cacheDir, cacheStats, cacheName, cachePath } = await getInfo({
    dir: opts.dir,
    pnpmHomeDir: opts.pnpmHomeDir,
    storeDir: opts.storeDir,
    cacheDir: opts.cacheDir,
    command,
  })
  const cleanExpiredCachePromise: Promise<void> = cleanExpiredCache({
    excludedCacheNames: [cacheName],
    cacheDir,
    cacheLifespanMillis: opts.dlxCacheMaxAge * 60_000,
    now: new Date(),
  })
  const tempPath = path.join(tempDir, `dlx-${process.pid.toString()}`)
  const prefix = cacheStats === 'ENOENT' ? tempPath : cachePath
  const modulesDir = path.join(prefix, 'node_modules')
  const binsDir = path.join(modulesDir, '.bin')
  process.on('exit', () => {
    if (opts.dlxCacheMaxAge <= 0) {
      try {
        fs.rmSync(tempPath, {
          recursive: true,
          maxRetries: 3,
        })
      } catch {}
    } else if (cacheStats === 'ENOENT') {
      fs.mkdirSync(path.dirname(cachePath), { recursive: true })
      fs.renameSync(tempPath, cachePath)
    }
  })
  const pkgs = opts.package ?? [command]
  const env = makeEnv({ userAgent: opts.userAgent, prependPaths: [binsDir] })
  if (cacheStats === 'ENOENT') {
    fs.mkdirSync(tempPath, { recursive: true })
    await add.handler({
      // Ideally the config reader should ignore these settings when the dlx command is executed.
      // This is a temporary solution until "@pnpm/config" is refactored.
      ...omit(['workspaceDir', 'rootProjectManifest'], opts),
      bin: binsDir,
      dir: tempPath,
      lockfileDir: tempPath,
      rootProjectManifestDir: tempPath, // This property won't be used as rootProjectManifest will be undefined
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
  await cleanExpiredCachePromise
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
  command: string
}) {
  const storeDir = await getStorePath({
    pkgRoot: opts.dir,
    storePath: opts.storeDir,
    pnpmHomeDir: opts.pnpmHomeDir,
  })
  const tempDir = path.join(storeDir, 'tmp')
  const cacheDir = path.resolve(opts.cacheDir, 'dlx')
  const cacheInfo = getCacheInfo(cacheDir, opts.command)
  return {
    storeDir,
    tempDir,
    cacheDir,
    ...cacheInfo,
  }
}

function getCacheInfo (cacheDir: string, command: string) {
  const cacheName = createBase32Hash(command)
  const cachePath = path.join(cacheDir, cacheName)
  let cacheStats: fs.Stats | 'ENOENT'
  try {
    cacheStats = fs.statSync(cachePath)
  } catch (err) {
    if (util.types.isNativeError(err) && 'code' in err && err.code === 'ENOENT') {
      cacheStats = 'ENOENT'
    } else {
      throw err
    }
  }
  return { cacheName, cachePath, cacheStats }
}

async function cleanExpiredCache (opts: {
  excludedCacheNames: string[],
  cacheDir: string,
  cacheLifespanMillis: number,
  now: Date
}): Promise<void> {
  const { excludedCacheNames, cacheDir, cacheLifespanMillis, now } = opts

  if (cacheLifespanMillis === Infinity) return

  let cacheItems: fs.Dirent[]
  try {
    cacheItems = await fs.promises.readdir(cacheDir, { encoding: 'utf-8', withFileTypes: true })
  } catch (err) {
    if (util.types.isNativeError(err) && 'code' in err && err.code === 'ENOENT') return
    throw err
  }
  await Promise.all(cacheItems.map(async item => {
    if (!item.isDirectory()) return
    if (excludedCacheNames.includes(item.name)) return
    const cachePath = path.join(cacheDir, item.name)
    let shouldClean: boolean
    if (cacheLifespanMillis <= 0) {
      shouldClean = true
    } else {
      const cacheStats = await fs.promises.stat(cachePath)
      shouldClean = cacheStats.ctime.getTime() + cacheLifespanMillis <= now.getTime()
    }
    if (shouldClean) {
      try {
        await fs.promises.rm(cachePath, { recursive: true })
      } catch {}
    }
  }))
}
