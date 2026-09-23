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

function manifestDeclaresNode (manifest: {
  dependencies?: Record<string, string>
  engines?: {
    runtime?: string | { name?: string } | Array<string | { name?: string }>
  }
}): boolean {
  if (manifest.dependencies?.node) return true
  const runtime = manifest.engines?.runtime
  if (typeof runtime === 'string') return runtime === 'node'
  if (Array.isArray(runtime)) {
    return runtime.some((entry) => (typeof entry === 'string' ? entry === 'node' : entry?.name === 'node'))
  }
  if (runtime && typeof runtime === 'object') {
    return runtime.name === 'node'
  }
  return false
}

async function getGlobalNodeInstalledVersion (globalPkgDir?: string, pnpmHomeDir?: string): Promise<string | null> {
  const globalDir = globalPkgDir ?? (pnpmHomeDir ? path.join(pnpmHomeDir, 'global', 'v11') : undefined)
  if (!globalDir) return null
  let entries: fs.Dirent[]
  try {
    entries = await fs.promises.readdir(globalDir, { withFileTypes: true })
  } catch (err) {
    if (util.types.isNativeError(err) && 'code' in err && err.code === 'ENOENT') {
      return null
    }
    throw err
  }
  const versions = await Promise.all(
    entries
      .filter((entry) => entry.isSymbolicLink())
      .map(async (entry) => {
        const linkPath = path.join(globalDir, entry.name)
        try {
          const installDir = await fs.promises.realpath(linkPath)
          let groupPkg: Record<string, unknown>
          try {
            groupPkg = JSON.parse(await fs.promises.readFile(path.join(installDir, 'package.json'), 'utf8'))
          } catch (err) {
            if (util.types.isNativeError(err) && 'code' in err && err.code === 'ENOENT') {
              return null
            }
            throw err
          }
          if (!manifestDeclaresNode(groupPkg)) return null
          const nodePkgJson = path.join(installDir, 'node_modules', 'node', 'package.json')
          const pkg = JSON.parse(await fs.promises.readFile(nodePkgJson, 'utf8'))
          return (pkg.version as string | undefined) ?? null
        } catch (err) {
          if (util.types.isNativeError(err) && 'code' in err && err.code === 'ENOENT') {
            return null
          }
          throw err
        }
      })
  )
  return versions.find(Boolean) ?? null
}

async function isDanglingSymlink (filePath: string): Promise<boolean> {
  try {
    await fs.promises.stat(filePath)
    return false
  } catch (err) {
    if (util.types.isNativeError(err) && 'code' in err && err.code === 'ENOENT') {
      return true
    }
    throw err
  }
}

async function isCandidateShimRemoved (binFile: string, ext: string, removedNames: Set<string>): Promise<boolean> {
  try {
    const stat = await fs.promises.lstat(binFile)
    if (stat.isSymbolicLink()) {
      const target = await fs.promises.readlink(binFile)
      const isDangling = await isDanglingSymlink(binFile)
      const targetSegments = target.split(/[\\/]/)
      return isDangling || Array.from(removedNames).some((name) => targetSegments.includes(name))
    }
    if (ext === '.cmd' || ext === '.ps1') {
      const content = await fs.promises.readFile(binFile, 'utf8')
      const segments = content.split(/["'\\/\s]+/)
      return Array.from(removedNames).some((name) => segments.includes(name))
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

  const installedGlobalNodeVersion = await getGlobalNodeInstalledVersion(opts.globalPkgDir, opts.pnpmHomeDir)
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
      entries = await fs.promises.readdir(nodejsDir)
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
      const stat = await fs.promises.lstat(nodeCurrentLink)
      if (stat.isSymbolicLink()) {
        const target = await fs.promises.readlink(nodeCurrentLink)
        const isDangling = await isDanglingSymlink(nodeCurrentLink)
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
    const extensions = process.platform === 'win32' ? ['', '.cmd', '.ps1', '.exe'] : ['']
    await Promise.all(
      ['node', 'npm', 'npx'].map(async (binBase) => {
        const candidates = extensions.map((ext) => ({
          ext,
          file: path.join(opts.bin!, `${binBase}${ext}`),
        }))
        const results = await Promise.all(
          candidates.map(async ({ file, ext }) => isCandidateShimRemoved(file, ext, removedNames))
        )
        if (results.some(Boolean)) {
          await Promise.all(
            candidates.map(async ({ file }) => {
              try {
                await fs.promises.lstat(file)
                await fs.promises.unlink(file)
                removedSomething = true
              } catch (err) {
                if (!util.types.isNativeError(err) || !('code' in err) || err.code !== 'ENOENT') {
                  throw err
                }
              }
            })
          )
        }
      })
    )
  }

  if (!removedSomething) {
    throw new PnpmError('ENV_NO_NODE_DIRECTORY', `Couldn't find Node.js version matching ${versions.join(', ')}`)
  }
}
