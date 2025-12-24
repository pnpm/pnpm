import { type Dirent, promises as fs } from 'fs'
import util from 'util'
import path from 'path'
import { readV8FileStrictAsync } from '@pnpm/fs.v8-file'
import { type PackageFilesIndex } from '@pnpm/store.cafs'
import { globalInfo, globalWarn } from '@pnpm/logger'
import rimraf from '@zkochan/rimraf'
import ssri from 'ssri'
import { getRegisteredProjects } from './projectRegistry.js'

const BIG_ONE = BigInt(1) as unknown
const LINKS_DIR = 'links'

export interface PruneOptions {
  cacheDir: string
  storeDir: string
}

export async function prune ({ cacheDir, storeDir }: PruneOptions, removeAlienFiles?: boolean): Promise<void> {
  // 1. First, prune the global virtual store
  // This must happen BEFORE pruning the CAS, because removing packages from
  // the virtual store will reduce hard link counts on files in the CAS
  await pruneGlobalVirtualStore(storeDir)

  // 2. Clean up metadata cache
  const metadataDirs = await getSubdirsSafely(cacheDir)
  await Promise.all(metadataDirs.map(async (metadataDir) => {
    if (!metadataDir.startsWith('metadata')) return
    try {
      await rimraf(path.join(cacheDir, metadataDir))
    } catch (err: unknown) {
      if (!(util.types.isNativeError(err) && 'code' in err && err.code === 'ENOENT')) {
        throw err
      }
    }
  }))
  await rimraf(path.join(storeDir, 'tmp'))
  globalInfo('Removed all cached metadata files')

  // 3. Prune the content-addressable store (CAS)
  const cafsDir = path.join(storeDir, 'files')
  const pkgIndexFiles = [] as string[]
  const indexDir = path.join(storeDir, 'index')
  await Promise.all((await getSubdirsSafely(indexDir)).map(async (dir) => {
    const subdir = path.join(indexDir, dir)
    await Promise.all((await fs.readdir(subdir)).map(async (fileName) => {
      const filePath = path.join(subdir, fileName)
      if (fileName.endsWith('.v8')) {
        pkgIndexFiles.push(filePath)
      }
    }))
  }))
  const removedHashes = new Set<string>()
  const dirs = await getSubdirsSafely(cafsDir)
  let fileCounter = 0
  await Promise.all(dirs.map(async (dir) => {
    const subdir = path.join(cafsDir, dir)
    await Promise.all((await fs.readdir(subdir)).map(async (fileName) => {
      const filePath = path.join(subdir, fileName)
      if (fileName.endsWith('.v8')) {
        pkgIndexFiles.push(filePath)
        return
      }
      const stat = await fs.stat(filePath)
      if (stat.isDirectory()) {
        if (removeAlienFiles) {
          await rimraf(filePath)
          globalWarn(`An alien directory has been removed from the store: ${filePath}`)
          fileCounter++
          return
        } else {
          globalWarn(`An alien directory is present in the store: ${filePath}`)
          return
        }
      }
      if (stat.nlink === 1 || stat.nlink === BIG_ONE) {
        await fs.unlink(filePath)
        fileCounter++
        removedHashes.add(ssri.fromHex(`${dir}${fileName}`, 'sha512').toString())
      }
    }))
  }))
  globalInfo(`Removed ${fileCounter} file${fileCounter === 1 ? '' : 's'}`)

  // 4. Clean up orphaned package index files
  let pkgCounter = 0
  await Promise.all(pkgIndexFiles.map(async (pkgIndexFilePath) => {
    const { files: pkgFilesIndex } = await readV8FileStrictAsync<PackageFilesIndex>(pkgIndexFilePath)
    // TODO: implement prune of Node.js packages, they don't have a package.json file
    if (pkgFilesIndex.has('package.json') && removedHashes.has(pkgFilesIndex.get('package.json')!.integrity)) {
      await fs.unlink(pkgIndexFilePath)
      pkgCounter++
    }
  }))
  globalInfo(`Removed ${pkgCounter} package${pkgCounter === 1 ? '' : 's'}`)
}

/**
 * Prune unused packages from the global virtual store using mark-and-sweep:
 * 1. Get all registered projects
 * 2. Find all node_modules directories in each project (including workspace packages)
 * 3. Walk symlinks from each node_modules to mark reachable packages
 * 4. Remove any package directories that weren't marked as reachable
 */
async function pruneGlobalVirtualStore (storeDir: string): Promise<void> {
  const linksDir = path.join(storeDir, LINKS_DIR)
  if (!await pathExists(linksDir)) {
    return
  }

  const projects = await getRegisteredProjects(storeDir)
  if (projects.length === 0) {
    globalInfo('No registered projects for global virtual store')
    return
  }

  globalInfo(`Checking ${projects.length} registered project(s) for global virtual store usage`)

  // Mark phase: collect all reachable package directories
  const reachable = new Set<string>()
  const visited = new Set<string>() // Track visited directories to prevent infinite loops

  // For each project, find all node_modules directories (root + workspace packages)
  await Promise.all(
    projects.map(async (projectDir) => {
      const nodeModulesDirs = await findAllNodeModulesDirs(projectDir)
      await Promise.all(
        nodeModulesDirs.map((modulesDir) =>
          walkSymlinksToStore(modulesDir, linksDir, reachable, visited)
        )
      )
    })
  )

  // Sweep phase: remove unreachable packages
  const unreachableCount = await removeUnreachablePackages(linksDir, reachable)
  if (unreachableCount > 0) {
    globalInfo(`Removed ${unreachableCount} package${unreachableCount === 1 ? '' : 's'} from global virtual store`)
  } else {
    globalInfo('No unused packages found in global virtual store')
  }
}

/**
 * Find all node_modules directories within a project, including those
 * in workspace packages. Does not descend into node_modules directories.
 */
async function findAllNodeModulesDirs (projectDir: string): Promise<string[]> {
  const nodeModulesDirs: string[] = []

  async function scan (dir: string): Promise<void> {
    let entries: Dirent[]
    try {
      entries = await fs.readdir(dir, { withFileTypes: true })
    } catch {
      return
    }

    const subdirs: string[] = []
    for (const entry of entries) {
      if (!entry.isDirectory()) continue

      const entryPath = path.join(dir, entry.name)

      if (entry.name === 'node_modules') {
        nodeModulesDirs.push(entryPath)
        // Don't descend into node_modules
      } else if (!entry.name.startsWith('.')) {
        // Collect directories to descend into (workspace packages, etc.)
        // Skip hidden directories like .git, .pnpm
        subdirs.push(entryPath)
      }
    }

    // Scan subdirectories concurrently
    await Promise.all(subdirs.map((subdir) => scan(subdir)))
  }

  await scan(projectDir)
  return nodeModulesDirs
}

/**
 * Recursively walk symlinks from a directory, marking any that point
 * into the global virtual store's links directory.
 */
async function walkSymlinksToStore (
  dir: string,
  linksDir: string,
  reachable: Set<string>,
  visited: Set<string>
): Promise<void> {
  // Prevent infinite loops from circular symlinks
  const realDir = await getRealPath(dir)
  if (visited.has(realDir)) {
    return
  }
  visited.add(realDir)

  let entries: Dirent[]
  try {
    entries = await fs.readdir(dir, { withFileTypes: true })
  } catch {
    return
  }

  await Promise.all(
    entries.map(async (entry) => {
      const entryPath = path.join(dir, entry.name)

      if (entry.isSymbolicLink()) {
        try {
          const target = await fs.readlink(entryPath)
          const absoluteTarget = path.isAbsolute(target)
            ? target
            : path.resolve(dir, target)

          // Check if this symlink points into the global virtual store
          if (absoluteTarget.startsWith(linksDir)) {
            // Mark the package directory as reachable
            // The path structure is: {linksDir}/{pkgName}/{version}/{hash}/node_modules/{pkgName}
            // We want to mark the {hash} directory
            const relPath = path.relative(linksDir, absoluteTarget)
            const parts = relPath.split(path.sep)
            // Find the hash directory (the one containing node_modules)
            const nodeModulesIdx = parts.indexOf('node_modules')
            if (nodeModulesIdx !== -1) {
              const hashDirPath = path.join(linksDir, ...parts.slice(0, nodeModulesIdx))
              reachable.add(hashDirPath)
              // Also walk into the package's node_modules for transitive deps
              const pkgNodeModules = path.join(hashDirPath, 'node_modules')
              await walkSymlinksToStore(pkgNodeModules, linksDir, reachable, visited)
            }
          }
        } catch {
          // Ignore broken symlinks
        }
      } else if (entry.isDirectory() && entry.name !== '.pnpm') {
        // Recurse into directories (but not .pnpm which is the local virtual store)
        await walkSymlinksToStore(entryPath, linksDir, reachable, visited)
      }
    })
  )
}

async function getRealPath (p: string): Promise<string> {
  try {
    return await fs.realpath(p)
  } catch {
    return p
  }
}

/**
 * Remove package directories from the global virtual store that are not in the reachable set.
 * Returns the count of removed packages.
 */
async function removeUnreachablePackages (
  linksDir: string,
  reachable: Set<string>
): Promise<number> {
  // Walk through the links directory structure: {linksDir}/{pkgName}/{version}/{hash}/
  const pkgNames = await getSubdirsSafely(linksDir)

  const results = await Promise.all(
    pkgNames.map(async (pkgName): Promise<number> => {
      const pkgDir = path.join(linksDir, pkgName)
      const versions = await getSubdirsSafely(pkgDir)

      let count = 0
      await Promise.all(
        versions.map(async (version) => {
          const versionDir = path.join(pkgDir, version)
          const hashes = await getSubdirsSafely(versionDir)

          // Remove unreachable hash directories
          await Promise.all(
            hashes.map(async (hash) => {
              const hashDir = path.join(versionDir, hash)
              if (!reachable.has(hashDir)) {
                await rimraf(hashDir)
                count++
              }
            })
          )

          // Clean up empty version directories
          const remainingHashes = await getSubdirsSafely(versionDir)
          if (remainingHashes.length === 0) {
            await rimraf(versionDir)
          }
        })
      )

      // Clean up empty package directories
      const remainingVersions = await getSubdirsSafely(pkgDir)
      if (remainingVersions.length === 0) {
        await rimraf(pkgDir)
      }

      return count
    })
  )

  return results.reduce((sum, count) => sum + count, 0)
}

async function pathExists (p: string): Promise<boolean> {
  try {
    await fs.stat(p)
    return true
  } catch {
    return false
  }
}

async function getSubdirsSafely (dir: string): Promise<string[]> {
  let entries: Dirent[]
  try {
    entries = await fs.readdir(dir, { withFileTypes: true }) as Dirent[]
  } catch (err: unknown) {
    if (util.types.isNativeError(err) && 'code' in err && err.code === 'ENOENT') {
      return []
    }
    throw err
  }
  return entries
    .filter(entry => entry.isDirectory())
    .map(dir => dir.name)
}
