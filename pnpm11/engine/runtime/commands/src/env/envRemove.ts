import fs from 'node:fs'
import path from 'node:path'
import util from 'node:util'

import { PnpmError } from '@pnpm/error'
import { runPnpmCli } from '@pnpm/exec.pnpm-cli-runner'
import { globalWarn } from '@pnpm/logger'

import type { NvmNodeCommandOptions } from './node.js'

function matchesNodeVersion (actualVersion: string, requestedVersion: string): boolean {
  return actualVersion === requestedVersion || actualVersion.startsWith(`${requestedVersion}.`)
}

function getGlobalNodeInstalledVersion (globalPkgDir?: string, pnpmHomeDir?: string): string | null {
  const globalDir = globalPkgDir ?? (pnpmHomeDir ? path.join(pnpmHomeDir, 'global', 'v11') : undefined)
  if (!globalDir) return null
  let entries: fs.Dirent[]
  try {
    entries = fs.readdirSync(globalDir, { withFileTypes: true })
  } catch (err) {
    if (util.types.isNativeError(err) && 'code' in err && err.code === 'ENOENT') {
      return null
    }
    throw err
  }
  for (const entry of entries) {
    if (!entry.isSymbolicLink()) continue
    const linkPath = path.join(globalDir, entry.name)
    let installDir: string
    try {
      installDir = fs.realpathSync(linkPath)
    } catch (err) {
      if (util.types.isNativeError(err) && 'code' in err && err.code === 'ENOENT') {
        continue
      }
      throw err
    }
    const nodePkgJson = path.join(installDir, 'node_modules', 'node', 'package.json')
    try {
      const pkg = JSON.parse(fs.readFileSync(nodePkgJson, 'utf8'))
      if (pkg.version) return pkg.version
    } catch (err) {
      if (util.types.isNativeError(err) && 'code' in err && err.code === 'ENOENT') {
        continue
      }
      throw err
    }
  }
  return null
}

function isCandidateShimRemoved (binFile: string, ext: string, removedNames: Set<string>): boolean {
  try {
    const stat = fs.lstatSync(binFile)
    if (stat.isSymbolicLink()) {
      const target = fs.readlinkSync(binFile)
      const isDangling = !fs.existsSync(binFile)
      const targetSegments = target.split(/[\\/]/)
      return isDangling || Array.from(removedNames).some((name) => targetSegments.includes(name))
    }
    if (ext === '.cmd' || ext === '.ps1') {
      const content = fs.readFileSync(binFile, 'utf8')
      return Array.from(removedNames).some((name) => content.includes(name))
    }
  } catch (err) {
    if (!util.types.isNativeError(err) || !('code' in err) || err.code !== 'ENOENT') {
      throw err
    }
  }
  return false
}

export async function envRemove (opts: NvmNodeCommandOptions, params: string[]): Promise<void> {
  globalWarn('"pnpm env remove" is deprecated. Use "pnpm remove -g node" instead.')
  if (!opts.global) {
    throw new PnpmError('NOT_IMPLEMENTED_YET', '"pnpm env remove <version>" can only be used with the "--global" option currently')
  }

  const versions = params.map((v) => v.trim()).filter(Boolean)
  if (versions.length === 0) {
    throw new PnpmError('MISSING_NODE_VERSION', '"pnpm env remove --global <version>" requires a Node.js version to be specified')
  }

  let removedSomething = false
  const removedNames = new Set<string>()

  const installedGlobalNodeVersion = getGlobalNodeInstalledVersion(opts.globalPkgDir, opts.pnpmHomeDir)
  const activeVersionMatches = installedGlobalNodeVersion != null &&
    versions.some((v) => matchesNodeVersion(installedGlobalNodeVersion, v))

  if (activeVersionMatches) {
    const args = ['remove', '--global', 'node']
    if (opts.bin) args.push('--global-bin-dir', opts.bin)
    if (opts.storeDir) args.push('--store-dir', opts.storeDir)
    if (opts.cacheDir) args.push('--cache-dir', opts.cacheDir)
    if (opts.globalDir) args.push('--global-dir', opts.globalDir)
    runPnpmCli(args, { cwd: opts.pnpmHomeDir })
    removedSomething = true
  }

  if (opts.pnpmHomeDir) {
    const nodejsDir = path.join(opts.pnpmHomeDir, 'nodejs')
    let entries: string[] = []
    try {
      entries = fs.readdirSync(nodejsDir)
    } catch (err) {
      if (!util.types.isNativeError(err) || !('code' in err) || err.code !== 'ENOENT') {
        throw err
      }
    }

    const entriesToRemove = entries.filter((entry) =>
      versions.some((v) => matchesNodeVersion(entry, v))
    )
    if (entriesToRemove.length > 0) {
      await Promise.all(
        entriesToRemove.map(async (entry) => {
          await fs.promises.rm(path.join(nodejsDir, entry), { recursive: true, force: true })
          removedNames.add(entry)
        })
      )
      removedSomething = true
    }

    const nodeCurrentLink = path.join(opts.pnpmHomeDir, 'nodejs_current')
    try {
      const stat = fs.lstatSync(nodeCurrentLink)
      if (stat.isSymbolicLink()) {
        const target = fs.readlinkSync(nodeCurrentLink)
        const isDangling = !fs.existsSync(nodeCurrentLink)
        const targetSegments = target.split(/[\\/]/)
        const pointsToRemoved = Array.from(removedNames).some((name) => targetSegments.includes(name))
        if (isDangling || pointsToRemoved) {
          await fs.promises.unlink(nodeCurrentLink)
          removedSomething = true
        }
      }
    } catch (err) {
      if (!util.types.isNativeError(err) || !('code' in err) || err.code !== 'ENOENT') {
        throw err
      }
    }
  }

  if (opts.bin) {
    const unlinks: Array<Promise<void>> = []
    const extensions = process.platform === 'win32' ? ['', '.cmd', '.ps1', '.exe'] : ['']
    for (const binBase of ['node', 'npm', 'npx']) {
      let shouldRemoveGroup = activeVersionMatches
      if (!shouldRemoveGroup) {
        for (const ext of extensions) {
          const binFile = path.join(opts.bin, `${binBase}${ext}`)
          if (isCandidateShimRemoved(binFile, ext, removedNames)) {
            shouldRemoveGroup = true
            break
          }
        }
      }
      if (shouldRemoveGroup) {
        for (const ext of extensions) {
          const binFile = path.join(opts.bin, `${binBase}${ext}`)
          try {
            fs.lstatSync(binFile)
            unlinks.push(fs.promises.unlink(binFile))
            removedSomething = true
          } catch (err) {
            if (!util.types.isNativeError(err) || !('code' in err) || err.code !== 'ENOENT') {
              throw err
            }
          }
        }
      }
    }
    await Promise.all(unlinks)
  }

  if (!removedSomething) {
    throw new PnpmError('ENV_NO_NODE_DIRECTORY', `Couldn't find Node.js version matching ${versions.join(', ')}`)
  }
}
