import { type Dirent, promises as fs } from 'fs'
import util from 'util'
import path from 'path'
import { createShortHash } from '@pnpm/crypto.hash'
import { PnpmError } from '@pnpm/error'
import { globalInfo } from '@pnpm/logger'
import symlinkDir from 'symlink-dir'

const PROJECTS_DIR = 'projects'

export function getProjectsRegistryDir (storeDir: string): string {
  return path.join(storeDir, PROJECTS_DIR)
}

/**
 * Register a project as using the store.
 * Creates a symlink in {storeDir}/projects/{hash} → {projectDir}
 */
export async function registerProject (storeDir: string, projectDir: string): Promise<void> {
  const registryDir = getProjectsRegistryDir(storeDir)
  await fs.mkdir(registryDir, { recursive: true })
  const linkPath = path.join(registryDir, createShortHash(projectDir))
  // symlink-dir handles the case where the symlink already exists
  await symlinkDir(projectDir, linkPath)
}

/**
 * Get all registered projects that use the global virtual store.
 * Cleans up stale entries (projects that no longer exist).
 */
export async function getRegisteredProjects (storeDir: string): Promise<string[]> {
  const registryDir = getProjectsRegistryDir(storeDir)
  let entries: Dirent[]
  try {
    entries = await fs.readdir(registryDir, { withFileTypes: true })
  } catch (err: unknown) {
    if (util.types.isNativeError(err) && 'code' in err && err.code === 'ENOENT') {
      return []
    }
    throw err
  }

  const projects: string[] = []
  await Promise.all(entries.map(async (entry) => {
    if (entry.name.startsWith('.')) return
    // We expect only symlinks (or junctions on Windows) in the registry
    if (!entry.isSymbolicLink()) return
    const linkPath = path.join(registryDir, entry.name)

    // Read the symlink target - if this fails, it's an invalid entry
    let target: string
    try {
      target = await fs.readlink(linkPath)
    } catch (err: unknown) {
      // If the file is not a symlink (EINVAL) or doesn't exist (ENOENT), ignore it
      if (util.types.isNativeError(err) && 'code' in err && (err.code === 'ENOENT' || err.code === 'EINVAL')) {
        return
      }
      // For permission errors etc, inform the user
      const message = util.types.isNativeError(err) ? err.message : String(err)
      throw new PnpmError('PROJECT_REGISTRY_ENTRY_INACCESSIBLE',
        `Cannot read project registry entry "${linkPath}": ${message}`,
        {
          hint: `To remove this project from the registry, delete the file at:\n  ${linkPath}`,
        }
      )
    }

    // Normalize to remove any trailing slashes (Windows junctions may include them)
    const absoluteTarget = path.normalize(path.isAbsolute(target) ? target : path.resolve(path.dirname(linkPath), target))

    // Check if project still exists
    try {
      await fs.stat(absoluteTarget)
      projects.push(absoluteTarget)
    } catch (err: unknown) {
      // Only clean up if project directory no longer exists
      if (util.types.isNativeError(err) && 'code' in err && err.code === 'ENOENT') {
        await fs.unlink(linkPath)
        globalInfo(`Removed stale project registry entry: ${absoluteTarget}`)
        return
      }
      // Can't access project - throw error to prevent incorrect pruning
      const message = util.types.isNativeError(err) ? err.message : String(err)
      throw new PnpmError('PROJECT_INACCESSIBLE',
        `Cannot access registered project "${absoluteTarget}": ${message}`,
        {
          hint: `To remove this project from the registry, delete the symlink at:\n  ${linkPath}`,
        }
      )
    }
  }))

  return projects
}
