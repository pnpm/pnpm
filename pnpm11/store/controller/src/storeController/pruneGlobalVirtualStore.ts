import crypto from 'node:crypto'
import { type Dirent, promises as fs, type Stats } from 'node:fs'
import path from 'node:path'

import { isStagingDirName } from '@pnpm/deps.graph-hasher'
import { isError } from '@pnpm/error'
import { globalInfo } from '@pnpm/logger'
import { rimraf } from '@zkochan/rimraf'
import { isSubdir } from 'is-subdir'

import { getRegisteredProjects } from './projectRegistry.js'

const LINKS_DIR = 'links'
// An install may still be building in a staging directory, or wait long for
// the build of the package staged there, so leftovers of crashed installs are
// left for a day.
const ABANDONED_STAGING_DIR_AGE_MS = 24 * 60 * 60 * 1000

/**
 * Prune unused packages from the global virtual store using mark-and-sweep:
 * 1. Get all registered projects
 * 2. Find all node_modules directories in each project (including workspace packages)
 * 3. Walk symlinks from each node_modules and from each of `modulesDirs` to mark reachable packages
 * 4. Remove any package directories that weren't marked as reachable, and the
 *    abandoned staging directories of the reachable ones
 */
export async function pruneGlobalVirtualStore (storeDir: string, modulesDirs: string[] = []): Promise<void> {
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
  await Promise.all(modulesDirs.map((modulesDir) => walkSymlinksToStore(modulesDir, linksDir, reachable, visited)))

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
    const entries = await readDirEntriesSafely(dir)
    if (!entries) return
    const subdirs: string[] = []
    for (const entry of entries) {
      if (!entry.isDirectory()) continue
      const entryPath = path.join(dir, entry.name)
      if (entry.name === 'node_modules') {
        nodeModulesDirs.push(entryPath)
      } else if (!entry.name.startsWith('.')) {
        subdirs.push(entryPath)
      }
    }
    await Promise.all(subdirs.map((subdir) => scan(subdir)))
  }

  await scan(projectDir)
  return nodeModulesDirs
}

async function readDirEntriesSafely (dir: string): Promise<Dirent[] | null> {
  try {
    return await fs.readdir(dir, { withFileTypes: true })
  } catch (err: unknown) {
    if (isError(err) && 'code' in err && (err.code === 'ENOENT' || err.code === 'ENOTDIR')) {
      return null
    }
    throw err
  }
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
  const dirHash = await getRealPathHash(dir)
  if (visited.has(dirHash)) {
    return
  }
  visited.add(dirHash)

  const entries = await readDirEntriesSafely(dir)
  if (!entries) return

  await Promise.all(
    entries.map((entry) => processDirEntry(entry, { dir, linksDir, reachable, visited }))
  )
}

interface ProcessDirEntryOptions {
  dir: string
  linksDir: string
  reachable: Set<string>
  visited: Set<string>
}

async function processDirEntry (
  entry: Dirent,
  { dir, linksDir, reachable, visited }: ProcessDirEntryOptions
): Promise<void> {
  const entryPath = path.join(dir, entry.name)
  if (entry.isSymbolicLink()) {
    try {
      await markSymlinkTargetReachable({ dir, entryPath, linksDir, reachable, visited })
    } catch {
      // Ignore broken symlinks
    }
  } else if (entry.isDirectory() && entry.name !== '.pnpm') {
    await walkSymlinksToStore(entryPath, linksDir, reachable, visited)
  }
}

interface MarkSymlinkTargetReachableOptions {
  dir: string
  entryPath: string
  linksDir: string
  reachable: Set<string>
  visited: Set<string>
}

async function markSymlinkTargetReachable ({ dir, entryPath, linksDir, reachable, visited }: MarkSymlinkTargetReachableOptions): Promise<void> {
  const target = await fs.readlink(entryPath)
  const absoluteTarget = path.isAbsolute(target)
    ? target
    : path.resolve(dir, target)

  if (!isSubdir(linksDir, absoluteTarget)) return
  // Mark the package directory as reachable
  // The path structure is:
  //   - Scoped:   {linksDir}/{scope}/{pkgName}/{version}/{hash}/node_modules/{pkgName}
  //   - Unscoped: {linksDir}/@/{pkgName}/{version}/{hash}/node_modules/{pkgName}
  // We want to mark the {hash} directory
  const relPath = path.relative(linksDir, absoluteTarget)
  const parts = relPath.split(path.sep)
  const nodeModulesIdx = parts.indexOf('node_modules')
  if (nodeModulesIdx === -1) return
  // Store relative path like "@scope/pkg-a/1.0.0/hash123" or "@/pkg-a/1.0.0/hash123"
  const relativePath = parts.slice(0, nodeModulesIdx).join(path.sep)
  reachable.add(relativePath)
  // Also walk into the package's node_modules for transitive deps
  const pkgNodeModules = path.join(linksDir, relativePath, 'node_modules')
  await walkSymlinksToStore(pkgNodeModules, linksDir, reachable, visited)
}

/**
 * Resolve symlinks and return a hash of the real path (for cycle detection)
 */
async function getRealPathHash (dir: string): Promise<string> {
  let realPath: string
  try {
    realPath = await fs.realpath(dir)
  } catch {
    realPath = dir
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
            await rimraf(pkgDir)
            removedPkgs++
          }
        })
      )

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
  pkgPath: string, // relative path like "@/is-positive" or "@pnpm.e2e/romeo"
  reachable: Set<string>
): Promise<{ count: number, allRemoved: boolean }> {
  const versions = await getSubdirsSafely(pkgDir)
  let count = 0
  let removedVersions = 0

  await Promise.all(
    versions.map(async (version) => {
      const versionDir = path.join(pkgDir, version)
      const hashes = await getSubdirsSafely(versionDir)

      let removedHashes = 0
      await Promise.all(
        hashes.map(async (hash) => {
          const relativePath = path.join(pkgPath, version, hash)
          if (!reachable.has(relativePath)) {
            await rimraf(path.join(versionDir, hash))
            removedHashes++
            count++
          } else {
            await removeAbandonedStagingDirs(path.join(versionDir, hash), pkgPath)
          }
        })
      )

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

/**
 * Removes the staging directories beside a slot's package directory that no
 * install has written to for {@link ABANDONED_STAGING_DIR_AGE_MS}. The package
 * directory is `node_modules/<name>` in the slot for an unscoped package (store
 * scope `@`) and `node_modules/<scope>/<name>` for a scoped one.
 */
async function removeAbandonedStagingDirs (slotDir: string, pkgPath: string): Promise<void> {
  const [scope] = pkgPath.split(path.sep)
  const parentDir = scope === '@'
    ? path.join(slotDir, 'node_modules')
    : path.join(slotDir, 'node_modules', scope)
  const entries = await readDirEntriesSafely(parentDir)
  if (!entries) return
  const abandonedBefore = Date.now() - ABANDONED_STAGING_DIR_AGE_MS
  await Promise.all(entries
    .filter((entry) => entry.isDirectory() && isStagingDirName(entry.name))
    .map(async (entry) => {
      const stagingDir = path.join(parentDir, entry.name)
      const changed = await changedSince(stagingDir, abandonedBefore)
      if (changed === false) await rimraf(stagingDir)
    }))
}

/**
 * Whether anything in the tree at `entryPath` changed after `cutoffMs`. A
 * build writes deep into its staging directory without touching the directory
 * itself, so every entry counts. Links are not followed. `undefined` when the
 * entry is gone, as when an install renamed its staging directory into place.
 */
async function changedSince (entryPath: string, cutoffMs: number): Promise<boolean | undefined> {
  let stats: Stats
  try {
    stats = await fs.lstat(entryPath)
  } catch (err: unknown) {
    if (isError(err) && 'code' in err && err.code === 'ENOENT') return undefined
    throw err
  }
  if (stats.mtimeMs > cutoffMs) return true
  if (!stats.isDirectory()) return false
  const entries = await readDirEntriesSafely(entryPath)
  for (const entry of entries ?? []) {
    // eslint-disable-next-line no-await-in-loop -- stops at the first entry that changed
    if (await changedSince(path.join(entryPath, entry.name), cutoffMs) !== false) return true
  }
  return false
}

async function pathExists (checkedPath: string): Promise<boolean> {
  try {
    await fs.stat(checkedPath)
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
    if (isError(err) && 'code' in err && err.code === 'ENOENT') {
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
