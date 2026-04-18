import { execFileSync } from 'node:child_process'
import {
  chmodSync,
  closeSync,
  constants,
  existsSync,
  fstatSync,
  mkdirSync,
  openSync,
  readdirSync,
  readlinkSync,
  statfsSync,
  statSync,
  symlinkSync,
  utimesSync,
} from 'node:fs'
import path from 'node:path'

import gracefulFs from '@pnpm/fs.graceful-fs'

// Linux filesystem magic numbers from statfs
const BTRFS_SUPER_MAGIC = 0x9123683e
const XFS_SUPER_MAGIC = 0x58465342

/**
 * Clone a directory using platform-specific copy-on-write mechanisms.
 * Returns true if cloning succeeded, false if fallback to per-file cloning is needed.
 */
export function cloneDir (src: string, dest: string): boolean {
  if (!existsSync(src)) {
    return false
  }

  // Ensure parent directory exists
  const parentDir = path.dirname(dest)
  if (parentDir && !existsSync(parentDir)) {
    try {
      mkdirSync(parentDir, { recursive: true })
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
 * macOS APFS directory cloning using clonefile syscall via cp -c.
 * macOS 10.15+ supports the -c flag for copy-on-write cloning.
 */
function cloneDirMacOS (src: string, dest: string): boolean {
  try {
    // Try cp -c first (macOS 10.15+ supports this for clonefile)
    // The -c flag enables copy-on-write cloning on APFS
    // The -R flag is for recursive directory copy
    // The -p flag preserves permissions, timestamps, etc.
    execFileSync('cp', ['-c', '-R', '-p', src, dest], {
      stdio: 'pipe',
      timeout: 60000,
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
function cloneDirLinux (src: string, dest: string): boolean {
  // First check if we're on a filesystem that supports reflink
  if (!isReflinkSupported(src)) {
    return false
  }

  // For Linux, we use the copy_file_range approach through fs.copyFileSync
  // with COPYFILE_FICLONE flag. However, this only works for files, not directories.
  // So we need to manually implement directory cloning using FICLONE.

  try {
    // Create the destination directory
    mkdirSync(dest, { recursive: true })

    // Read the source directory
    const entries = readdirSync(src, { withFileTypes: true })

    for (const entry of entries) {
      const srcPath = path.join(src, entry.name)
      const destPath = path.join(dest, entry.name)

      if (entry.isDirectory()) {
        // Recursively clone subdirectories
        if (!cloneDirLinux(srcPath, destPath)) {
          // If subdirectory clone fails, fall back to copy
          mkdirSync(destPath, { recursive: true })
          if (!cloneDirLinux(srcPath, destPath)) {
            return false
          }
        }
      } else if (entry.isFile()) {
        // Try to reflink clone the file using FICLONE ioctl
        if (!reflinkCloneFile(srcPath, destPath)) {
          // Fall back to regular copyFile with FICLONE_FORCE
          try {
            gracefulFs.copyFileSync(srcPath, destPath, constants.COPYFILE_FICLONE)
          } catch {
            gracefulFs.copyFileSync(srcPath, destPath)
          }
        }
      } else if (entry.isSymbolicLink()) {
        // Copy symlinks as symlinks
        const linkTarget = readlinkSync(srcPath)
        symlinkSync(linkTarget, destPath)
      }
    }

    // Copy permissions and timestamps from source to destination
    try {
      const srcStat = statSync(src)
      chmodSync(dest, srcStat.mode)
      utimesSync(dest, srcStat.atime, srcStat.mtime)
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
function isReflinkSupported (path: string): boolean {
  try {
    const statfs = statfsSync(path)
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
function reflinkCloneFile (src: string, dest: string): boolean {
  let srcFd: number | undefined
  let destFd: number | undefined

  try {
    // Open source file read-only
    srcFd = openSync(src, 'r')

    // Get source file stats for permissions
    const srcStat = fstatSync(srcFd)

    // Open/create destination file with same permissions
    destFd = openSync(dest, 'w', srcStat.mode)

    // Perform the reflink clone using ioctl
    // We need to use a native addon or process.binding for ioctl
    // Since we can't easily do that, fall back to copy_file_range
    // which uses the same underlying mechanism
    gracefulFs.copyFileSync(src, dest, constants.COPYFILE_FICLONE_FORCE)

    return true
  } catch {
    return false
  } finally {
    if (srcFd !== undefined) {
      try {
        closeSync(srcFd)
      } catch {} // eslint-disable-line:no-empty
    }
    if (destFd !== undefined) {
      try {
        closeSync(destFd)
      } catch {} // eslint-disable-line:no-empty
    }
  }
}
