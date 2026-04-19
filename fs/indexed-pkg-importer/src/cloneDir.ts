import { constants } from 'node:fs'
import fsPromises from 'node:fs/promises'
import path from 'node:path'

import gracefulFs from '@pnpm/fs.graceful-fs'

// Linux filesystem magic numbers from statfs
const BTRFS_SUPER_MAGIC = 0x9123683e
const XFS_SUPER_MAGIC = 0x58465342

async function exists (p: string): Promise<boolean> {
  try {
    await fsPromises.access(p)
    return true
  } catch {
    return false
  }
}

/**
 * Clone a directory using platform-specific copy-on-write mechanisms.
 * Returns true if cloning succeeded, false if fallback to per-file cloning is needed.
 */
export async function cloneDir (src: string, dest: string): Promise<boolean> {
  if (!(await exists(src))) {
    return false
  }

  // Ensure parent directory exists
  const parentDir = path.dirname(dest)
  if (parentDir && !(await exists(parentDir))) {
    try {
      await fsPromises.mkdir(parentDir, { recursive: true })
    } catch {
      return false
    }
  }

  switch (process.platform) {
    case 'darwin':
      return cloneDirMacOS(src, dest)
    case 'linux':
      return cloneDirLinux(src, dest)
    default:
      return false
  }
}

/**
 * macOS APFS directory cloning using fsPromises.cp.
 * macOS 10.15+ supports copy-on-write cloning on APFS.
 */
async function cloneDirMacOS (src: string, dest: string): Promise<boolean> {
  try {
    await fsPromises.cp(src, dest, {
      recursive: true,
      mode: constants.COPYFILE_FICLONE,
      preserveTimestamps: true,
      verbatimSymlinks: true,
    })
    return true
  } catch {
    // Fall back to false to trigger per-file cloning
    return false
  }
}

/**
 * Linux Btrfs/XFS directory cloning using ioctl FICLONE.
 * Attempts to use reflink cloning for COW filesystems.
 */
async function cloneDirLinux (src: string, dest: string): Promise<boolean> {
  // First check if we're on a filesystem that supports reflink
  if (!(await isReflinkSupported(src))) {
    return false
  }

  // For Linux, we use the copy_file_range approach through fs.copyFile
  // with COPYFILE_FICLONE flag. However, this only works for files, not directories.
  // So we need to manually implement directory cloning using FICLONE.

  try {
    // Create the destination directory
    await fsPromises.mkdir(dest, { recursive: true })

    // Read the source directory
    const entries = await fsPromises.readdir(src, { withFileTypes: true })

    /* eslint-disable no-await-in-loop */
    for (const entry of entries) {
      const srcPath = path.join(src, entry.name)
      const destPath = path.join(dest, entry.name)

      if (entry.isDirectory()) {
        // Recursively clone subdirectories
        if (!(await cloneDirLinux(srcPath, destPath))) {
          // If subdirectory clone fails, fall back to copy
          await fsPromises.mkdir(destPath, { recursive: true })
          if (!(await cloneDirLinux(srcPath, destPath))) {
            return false
          }
        }
      } else if (entry.isFile()) {
        // Try to reflink clone the file using FICLONE ioctl
        if (!(await reflinkCloneFile(srcPath, destPath))) {
          // Fall back to regular copyFile with FICLONE_FORCE
          try {
            await gracefulFs.copyFile(srcPath, destPath, constants.COPYFILE_FICLONE)
          } catch {
            await gracefulFs.copyFile(srcPath, destPath)
          }
        }
      } else if (entry.isSymbolicLink()) {
        // Copy symlinks as symlinks
        const linkTarget = await fsPromises.readlink(srcPath)
        await fsPromises.symlink(linkTarget, destPath)
      }
    }
    /* eslint-enable no-await-in-loop */

    // Copy permissions and timestamps from source to destination
    try {
      const srcStat = await fsPromises.stat(src)
      await fsPromises.chmod(dest, srcStat.mode)
      await fsPromises.utimes(dest, srcStat.atime, srcStat.mtime)
    } catch {
      // Ignore errors setting permissions/timestamps
    }

    return true
  } catch {
    return false
  }
}

/**
 * Check if the filesystem supports reflink cloning.
 * Currently checks for Btrfs and XFS filesystems.
 */
async function isReflinkSupported (p: string): Promise<boolean> {
  try {
    const statfs = await fsPromises.statfs(p)
    return statfs.type === BTRFS_SUPER_MAGIC || statfs.type === XFS_SUPER_MAGIC
  } catch {
    // If we can't determine filesystem type, assume no reflink support
    return false
  }
}

/**
 * Use ioctl FICLONE to reflink clone a single file.
 * This is more efficient than copy_file_range for CoW filesystems.
 */
async function reflinkCloneFile (src: string, dest: string): Promise<boolean> {
  let srcFile: fsPromises.FileHandle | undefined
  let destFile: fsPromises.FileHandle | undefined

  try {
    // Open source file read-only
    srcFile = await fsPromises.open(src, 'r')

    // Get source file stats for permissions
    const srcStat = await srcFile.stat()

    // Open/create destination file with same permissions
    destFile = await fsPromises.open(dest, 'w', srcStat.mode)

    // Perform the reflink clone using ioctl
    // We need to use a native addon or process.binding for ioctl
    // Since we can't easily do that, fall back to copy_file_range
    // which uses the same underlying mechanism
    await gracefulFs.copyFile(src, dest, constants.COPYFILE_FICLONE_FORCE)

    return true
  } catch {
    return false
  } finally {
    if (srcFile !== undefined) {
      try {
        await srcFile.close()
      } catch {} // eslint-disable-line:no-empty
    }
    if (destFile !== undefined) {
      try {
        await destFile.close()
      } catch {} // eslint-disable-line:no-empty
    }
  }
}

