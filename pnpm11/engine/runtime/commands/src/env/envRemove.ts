import fs from 'node:fs'
import path from 'node:path'
import util from 'node:util'

import { isError, PnpmError } from '@pnpm/error'
import { removeGlobalGroups } from '@pnpm/global.commands'
import { scanGlobalPackages } from '@pnpm/global.packages'
import { globalWarn } from '@pnpm/logger'

import type { NvmNodeCommandOptions } from './node.js'

const realpathJs = util.promisify(fs.realpath)

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
    return enginesDeclareNode(manifest.engines.runtime)
  }
  return false
}

function enginesDeclareNode (runtime: unknown): boolean {
  if (typeof runtime === 'string') return runtime === 'node'
  if (Array.isArray(runtime)) {
    return runtime.some((entry) => isNodeRuntimeEntry(entry))
  }
  if (isRecord(runtime)) {
    return runtime.name === 'node'
  }
  return false
}

function isNodeRuntimeEntry (entry: unknown): boolean {
  if (typeof entry === 'string') return entry === 'node'
  if (isRecord(entry)) return entry.name === 'node'
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
    if (isError(err) && 'code' in err && err.code === 'ENOENT') {
      return null
    }
    throw err
  }
  const groups = await Promise.all(
    entries
      .filter((entry) => entry.isSymbolicLink())
      .map(async (entry) => readGlobalNodeGroup(globalDir, entry.name))
  )
  return groups.find((group) => group != null) ?? null
}

async function readGlobalNodeGroup (globalDir: string, entryName: string): Promise<GlobalNodeGroup | null> {
  const linkPath = path.join(globalDir, entryName)
  try {
    const installDir = await realpathJs(linkPath)
    const groupPkg = await tryReadJsonFile(path.join(installDir, 'package.json'))
    if (!manifestDeclaresNode(groupPkg)) return null
    const nodePkg = await tryReadJsonFile(path.join(installDir, 'node_modules', 'node', 'package.json'))
    const version = (nodePkg as { version?: string })?.version
    return version ? { hash: entryName, installDir, version } : null
  } catch (err) {
    if (isError(err) && 'code' in err && err.code === 'ENOENT') {
      return null
    }
    throw err
  }
}

async function tryReadJsonFile (filePath: string): Promise<unknown> {
  try {
    return JSON.parse(await fs.promises.readFile(filePath, 'utf8'))
  } catch (err) {
    if (isError(err) && 'code' in err && err.code === 'ENOENT') {
      return null
    }
    throw err
  }
}

async function isDanglingSymlink (filePath: string): Promise<boolean> {
  try {
    await fs.promises.stat(filePath)
    return false
  } catch (err) {
    if (isError(err) && 'code' in err && err.code === 'ENOENT') {
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
    if (!isError(err) || !('code' in err) || err.code !== 'ENOENT') {
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

  const removedNames = new Set<string>()
  const removedGlobal = await removeGlobalNodePkg(opts, versions)
  const removedVersions = await removeNodejsVersions(opts.pnpmHomeDir, versions, removedNames)
  const removedShims = await removeBinShims(opts.bin, removedNames)

  if (!removedGlobal && !removedVersions && !removedShims) {
    throw new PnpmError('ENV_NO_NODE_DIRECTORY', `Couldn't find Node.js version matching ${versions.join(', ')}`)
  }
}

async function removeGlobalNodePkg (opts: NvmNodeCommandOptions, versions: string[]): Promise<boolean> {
  const globalPkgDir = opts.globalPkgDir ?? (opts.pnpmHomeDir ? path.join(opts.pnpmHomeDir, 'global', 'v11') : undefined)
  const globalNode = globalPkgDir ? await findGlobalNodeGroup(globalPkgDir) : null
  if (globalPkgDir && globalNode && versions.some((version) => matchesNodeVersion(globalNode.version, version))) {
    const group = scanGlobalPackages(globalPkgDir).find(({ hash }) => hash === globalNode.hash)
    await removeGlobalGroups({ globalPkgDir, bin: opts.bin }, [{
      hash: globalNode.hash,
      installDir: globalNode.installDir,
      dependencies: { ...group?.dependencies, node: globalNode.version },
    }])
    return true
  }
  return false
}

async function removeNodejsVersions (
  pnpmHomeDir: string | undefined,
  versions: string[],
  removedNames: Set<string>
): Promise<boolean> {
  if (!pnpmHomeDir) return false
  let removedSomething = false
  const nodejsDir = path.join(pnpmHomeDir, 'nodejs')
  const entries = await tryReadDir(nodejsDir)
  const entriesToRemove = entries.filter((entry) =>
    versions.some((version) => matchesNodeVersion(entry, version))
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

  const removedCurrentLink = await unlinkIfCurrentLinkMatches(pnpmHomeDir, removedNames)
  return removedSomething || removedCurrentLink
}

async function tryReadDir (dirPath: string): Promise<string[]> {
  try {
    return await fs.promises.readdir(dirPath)
  } catch (err) {
    if (!isError(err) || !('code' in err) || err.code !== 'ENOENT') {
      throw err
    }
    return []
  }
}

async function unlinkIfCurrentLinkMatches (pnpmHomeDir: string, removedNames: Set<string>): Promise<boolean> {
  const nodeCurrentLink = path.join(pnpmHomeDir, 'nodejs_current')
  try {
    const stat = await fs.promises.lstat(nodeCurrentLink)
    if (!stat.isSymbolicLink()) return false
    const target = await fs.promises.readlink(nodeCurrentLink)
    const isDangling = await isDanglingSymlink(nodeCurrentLink)
    const targetSegments = target.split(/[\\/]/)
    const pointsToRemoved = Array.from(removedNames).some((name) => targetSegments.includes(name))
    if (isDangling || pointsToRemoved) {
      await fs.promises.unlink(nodeCurrentLink)
      return true
    }
  } catch (err) {
    if (!isError(err) || !('code' in err) || err.code !== 'ENOENT') {
      throw err
    }
  }
  return false
}

async function removeBinShims (binDir: string | undefined, removedNames: Set<string>): Promise<boolean> {
  if (!binDir) return false
  const extensions = process.platform === 'win32' ? ['', '.cmd', '.ps1', '.exe'] : ['']
  const results = await Promise.all(
    ['node', 'npm', 'npx'].map(async (binBase) =>
      cleanBinBaseShims(binDir, binBase, extensions, removedNames)
    )
  )
  return results.some(Boolean)
}

async function cleanBinBaseShims (
  binDir: string,
  binBase: string,
  extensions: string[],
  removedNames: Set<string>
): Promise<boolean> {
  const candidates = extensions.map((ext) => ({
    ext,
    file: path.join(binDir, `${binBase}${ext}`),
  }))
  const checks = await Promise.all(
    candidates.map(async ({ file, ext }) => isCandidateShimRemoved(file, ext, removedNames))
  )
  if (!checks.some(Boolean)) {
    return false
  }
  const unlinks = await Promise.all(
    candidates.map(async ({ file }) => tryUnlink(file))
  )
  return unlinks.some(Boolean)
}

async function tryUnlink (file: string): Promise<boolean> {
  try {
    await fs.promises.lstat(file)
    await fs.promises.unlink(file)
    return true
  } catch (err) {
    if (!isError(err) || !('code' in err) || err.code !== 'ENOENT') {
      throw err
    }
    return false
  }
}
