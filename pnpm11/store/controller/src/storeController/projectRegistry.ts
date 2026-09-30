import { type Dirent, promises as fs } from 'node:fs'
import path from 'node:path'

import { createShortHash } from '@pnpm/crypto.hash'
import { isError, PnpmError } from '@pnpm/error'
import { globalInfo } from '@pnpm/logger'
import { isSubdir } from 'is-subdir'
import { symlinkDir } from 'symlink-dir'

const PROJECTS_DIR = 'projects'

export function getProjectsRegistryDir (storeDir: string): string {
  return path.join(storeDir, PROJECTS_DIR)
}

/**
 * Register a project as using the store.
 * Creates a symlink in {storeDir}/projects/{hash} → {projectDir}
 */
export async function registerProject (storeDir: string, projectDir: string): Promise<void> {
  // Avoid creating circular symlinks when the store is inside the project directory
  if (isSubdir(projectDir, storeDir)) {
    return
  }
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
  const entries = await readRegistryDir(registryDir)
  const projects: string[] = []
  await Promise.all(entries.map(async (entry) => {
    const project = await resolveRegisteredProject(registryDir, entry)
    if (project != null) {
      projects.push(project)
    }
  }))

  return projects
}

async function readRegistryDir (registryDir: string): Promise<Dirent[]> {
  try {
    return await fs.readdir(registryDir, { withFileTypes: true })
  } catch (err: unknown) {
    if (isError(err) && 'code' in err && err.code === 'ENOENT') {
      return []
    }
    throw err
  }
}

async function resolveRegisteredProject (registryDir: string, entry: Dirent): Promise<string | undefined> {
  if (entry.name.startsWith('.')) return undefined
  // We expect only symlinks (or junctions on Windows) in the registry
  if (!entry.isSymbolicLink()) return undefined
  const linkPath = path.join(registryDir, entry.name)
  const target = await readRegistryEntry(linkPath)
  if (target == null) return undefined
  const absoluteTarget = path.isAbsolute(target) ? target : path.resolve(path.dirname(linkPath), target)
  return await registeredProjectExists(linkPath, absoluteTarget) ? absoluteTarget : undefined
}

/**
 * Reads the symlink target of a registry entry, or returns `undefined` when
 * the entry is not a symlink or no longer exists.
 */
async function readRegistryEntry (linkPath: string): Promise<string | undefined> {
  try {
    return await fs.readlink(linkPath)
  } catch (err: unknown) {
    // If the file is not a symlink (EINVAL) or doesn't exist (ENOENT), ignore it
    if (isError(err) && 'code' in err && (err.code === 'ENOENT' || err.code === 'EINVAL')) {
      return undefined
    }
    // For permission errors etc, inform the user
    const message = isError(err) ? err.message : String(err)
    throw new PnpmError('PROJECT_REGISTRY_ENTRY_INACCESSIBLE',
      `Cannot read project registry entry "${linkPath}": ${message}`,
      {
        hint: `To remove this project from the registry, delete the file at:\n  ${linkPath}`,
      }
    )
  }
}

/**
 * Removes the registry entry of a project directory that no longer exists
 * and throws if the project directory cannot be accessed.
 */
async function registeredProjectExists (linkPath: string, absoluteTarget: string): Promise<boolean> {
  try {
    await fs.stat(absoluteTarget)
    return true
  } catch (err: unknown) {
    // Only clean up if project directory no longer exists
    if (isError(err) && 'code' in err && err.code === 'ENOENT') {
      await fs.unlink(linkPath)
      globalInfo(`Removed stale project registry entry: ${absoluteTarget}`)
      return false
    }
    // Can't access project - throw error to prevent incorrect pruning
    const message = isError(err) ? err.message : String(err)
    throw new PnpmError('PROJECT_INACCESSIBLE',
      `Cannot access registered project "${absoluteTarget}": ${message}`,
      {
        hint: `To remove this project from the registry, delete the symlink at:\n  ${linkPath}`,
      }
    )
  }
}
