import { promises as fs } from 'fs'
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
  let entries: string[]
  try {
    entries = await fs.readdir(registryDir)
  } catch (err: unknown) {
    if (util.types.isNativeError(err) && 'code' in err && err.code === 'ENOENT') {
      return []
    }
    throw err
  }

  const projects: string[] = []
  await Promise.all(entries.map(async (entry) => {
    const linkPath = path.join(registryDir, entry)
    try {
      const target = await fs.readlink(linkPath)
      const absoluteTarget = path.isAbsolute(target) ? target : path.resolve(path.dirname(linkPath), target)
      // Check if project still exists
      try {
        await fs.stat(absoluteTarget)
        projects.push(absoluteTarget)
      } catch (err: unknown) {
        // Only clean up if project directory no longer exists
        if (util.types.isNativeError(err) && 'code' in err && err.code === 'ENOENT') {
          await fs.unlink(linkPath)
          globalInfo(`Removed stale project registry entry: ${absoluteTarget}`)
        } else {
          // Can't access project - throw error to prevent incorrect pruning
          const message = util.types.isNativeError(err) ? err.message : String(err)
          throw new PnpmError('PROJECT_INACCESSIBLE',
            `Cannot access registered project "${absoluteTarget}": ${message}`,
            {
              hint: `To remove this project from the registry, delete the symlink at:\n  ${linkPath}`,
            }
          )
        }
      }
    } catch {
      // Invalid symlink, remove it
      try {
        await fs.unlink(linkPath)
      } catch {
        // Ignore errors when removing invalid entries
      }
    }
  }))

  return projects
}
