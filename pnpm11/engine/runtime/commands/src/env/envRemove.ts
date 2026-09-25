import fs from 'node:fs'
import path from 'node:path'
import util from 'node:util'

import { PnpmError } from '@pnpm/error'
import { removeGlobalGroups } from '@pnpm/global.commands'
import { scanGlobalPackages } from '@pnpm/global.packages'
import { globalWarn } from '@pnpm/logger'

import type { NvmNodeCommandOptions } from './node.js'

function matchesNodeVersion (actualVersion: string, requestedVersion: string): boolean {
  return actualVersion === requestedVersion || actualVersion.startsWith(`${requestedVersion}.`)
}

function isRecord (value: unknown): value is Record<string, unknown> {
  return value !== null && typeof value === 'object' && !Array.isArray(value)
}

function manifestDeclaresNode (manifest: unknown): boolean {
  if (!isRecord(manifest)) return false
  if (isRecord(manifest.dependencies) && 'node' in manifest.dependencies) return true
  if (isRecord(manifest.engines)) {
    const runtime = manifest.engines.runtime
    if (typeof runtime === 'string') return runtime === 'node'
    if (Array.isArray(runtime)) {
      return runtime.some((entry) => {
        if (typeof entry === 'string') return entry === 'node'
        if (isRecord(entry)) return entry.name === 'node'
        return false
      })
    }
    if (isRecord(runtime)) {
      return runtime.name === 'node'
    }
  }
  return false
}

interface GlobalNodeGroup {
  hash: string
  installDir: string
  version: string
}

async function findGlobalNodeGroup (globalDir: string): Promise<GlobalNodeGroup | null> {
  let entries: fs.Dirent[]
  try {
    entries = await fs.promises.readdir(globalDir, { withFileTypes: true })
  } catch (err) {
    if (util.types.isNativeError(err) && 'code' in err && err.code === 'ENOENT') {
      return null
    }
    throw err
  }
  const groups = await Promise.all(
    entries
      .filter((entry) => entry.isSymbolicLink())
      .map(async (entry) => {
        const linkPath = path.join(globalDir, entry.name)
        try {
          // The JS realpath, like scanGlobalPackages, keeps Windows 8.3 short
          // names, so the install dir still lies under the configured global dir.
          const installDir = fs.realpathSync(linkPath)
          let groupPkg: unknown
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
          const version = pkg.version as string | undefined
          return version ? { hash: entry.name, installDir, version } : null
        } catch (err) {
          if (util.types.isNativeError(err) && 'code' in err && err.code === 'ENOENT') {
            return null
          }
          throw err
        }
      })
  )
  return groups.find((group) => group != null) ?? null
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

  const globalPkgDir = opts.globalPkgDir ?? (opts.pnpmHomeDir ? path.join(opts.pnpmHomeDir, 'global', 'v11') : undefined)
  const globalNode = globalPkgDir ? await findGlobalNodeGroup(globalPkgDir) : null
  if (globalPkgDir && globalNode && versions.some((v) => matchesNodeVersion(globalNode.version, v))) {
    // In-process rather than through `pnpm remove --global`, which refuses to
    // run when the global bin directory is not on PATH.
    // The group may hold other packages, whose bins go with its install dir.
    const group = scanGlobalPackages(globalPkgDir).find(({ hash }) => hash === globalNode.hash)
    await removeGlobalGroups({ globalPkgDir, bin: opts.bin }, [{
      hash: globalNode.hash,
      installDir: globalNode.installDir,
      dependencies: { ...group?.dependencies, node: globalNode.version },
    }])
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
