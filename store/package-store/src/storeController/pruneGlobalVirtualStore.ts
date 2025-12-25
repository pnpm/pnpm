import { type Dirent, promises as fs } from 'fs'
import util from 'util'
import path from 'path'
import crypto from 'crypto'
import { globalInfo } from '@pnpm/logger'
import rimraf from '@zkochan/rimraf'
import { getRegisteredProjects } from './projectRegistry.js'

const LINKS_DIR = 'links'

/**
 * Prune unused packages from the global virtual store using mark-and-sweep:
 * 1. Get all registered projects
 * 2. Find all node_modules directories in each project (including workspace packages)
 * 3. Walk symlinks from each node_modules to mark reachable packages
 * 4. Remove any package directories that weren't marked as reachable
 */
export async function pruneGlobalVirtualStore (storeDir: string): Promise<void> {
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
  const dirHash = await getRealPathHash(dir)
  if (visited.has(dirHash)) {
    return
  }
  visited.add(dirHash)

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
              // Store relative path like "pkg-a/1.0.0/hash123"
              const relativePath = parts.slice(0, nodeModulesIdx).join(path.sep)
              reachable.add(relativePath)
              // Also walk into the package's node_modules for transitive deps
              const pkgNodeModules = path.join(linksDir, relativePath, 'node_modules')
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

/**
 * Resolve symlinks and return a hash of the real path (for cycle detection)
 */
async function getRealPathHash (p: string): Promise<string> {
  let realPath: string
  try {
    realPath = await fs.realpath(p)
  } catch {
    realPath = p
  }
  // Create a compact hash for in-memory use (base64url is shorter than hex that we use for file name hashes)
  return crypto.createHash('sha256').update(realPath).digest('base64url')
}

/**
 * Remove package directories from the global virtual store that are not in the reachable set.
 * Returns the count of removed packages.
 *
 * Directory structure is uniform 4-level:
 * - Scoped: {linksDir}/{scope}/{pkgName}/{version}/{hash}/
 * - Unscoped: {linksDir}/@/{pkgName}/{version}/{hash}/
 */
async function removeUnreachablePackages (
  linksDir: string,
  reachable: Set<string>
): Promise<number> {
  // First level is always a scope (either @scope or @ for unscoped packages)
  const scopes = await getSubdirsSafely(linksDir)
  let count = 0

  await Promise.all(
    scopes.map(async (scope) => {
      const scopePath = path.join(linksDir, scope)
      const pkgNames = await getSubdirsSafely(scopePath)
      let removedPkgs = 0

      await Promise.all(
        pkgNames.map(async (pkgName) => {
          const pkgDir = path.join(scopePath, pkgName)
          const removedVersions = await removeUnreachableVersions(
            pkgDir,
            path.join(scope, pkgName),
            reachable
          )
          count += removedVersions.count
          if (removedVersions.allRemoved) {
            // Remove the package directory when all its versions are removed
            await rimraf(pkgDir)
            removedPkgs++
          }
        })
      )

      // If we removed all packages in scope, remove the scope directory
      if (removedPkgs === pkgNames.length && pkgNames.length > 0) {
        await rimraf(scopePath)
      }
    })
  )

  return count
}

/**
 * Remove unreachable versions and hashes for a package.
 * Returns the count of removed packages and whether all versions were removed.
 */
async function removeUnreachableVersions (
  pkgDir: string,
  pkgPath: string, // relative path like "is-positive" or "@pnpm.e2e/romeo"
  reachable: Set<string>
): Promise<{ count: number, allRemoved: boolean }> {
  const versions = await getSubdirsSafely(pkgDir)
  let count = 0
  let removedVersions = 0

  await Promise.all(
    versions.map(async (version) => {
      const versionDir = path.join(pkgDir, version)
      const hashes = await getSubdirsSafely(versionDir)

      // Remove unreachable hash directories
      let removedHashes = 0
      await Promise.all(
        hashes.map(async (hash) => {
          const relativePath = path.join(pkgPath, version, hash)
          if (!reachable.has(relativePath)) {
            await rimraf(path.join(versionDir, hash))
            removedHashes++
            count++
          }
        })
      )

      // If we removed all hashes, remove the version directory
      if (removedHashes === hashes.length && hashes.length > 0) {
        await rimraf(versionDir)
        removedVersions++
      }
    })
  )

  return {
    count,
    allRemoved: removedVersions === versions.length && versions.length > 0,
  }
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
  const subdirs: string[] = []
  for (const entry of entries) {
    if (entry.isDirectory()) {
      subdirs.push(entry.name)
    }
  }
  return subdirs
}
