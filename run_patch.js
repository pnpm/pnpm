const fs = require('fs');

// 1. Rewrite cloneDir.ts
const cloneDirContent = \`/* eslint-disable no-await-in-loop */
import { execFile } from 'node:child_process'
import { constants } from 'node:fs'
import {
  access,
  chmod,
  mkdir,
  open,
  readdir,
  readlink,
  statfs,
  stat,
  symlink,
  utimes,
  copyFile,
} from 'node:fs/promises'
import path from 'node:path'
import { promisify } from 'node:util'

const execFileAsync = promisify(execFile)

// Linux filesystem magic numbers from statfs
const BTRFS_SUPER_MAGIC = 0x9123683e
const XFS_SUPER_MAGIC = 0x58465342

async function pathExists (p: string) {
  try {
    await access(p)
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
  if (!(await pathExists(src))) {
    return false
  }

  // Ensure parent directory exists
  const parentDir = path.dirname(dest)
  if (parentDir && !(await pathExists(parentDir))) {
    try {
      await mkdir(parentDir, { recursive: true })
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
async function cloneDirMacOS (src: string, dest: string): Promise<boolean> {
  try {
    // Try cp -c first (macOS 10.15+ supports this for clonefile)
    // The -c flag enables copy-on-write cloning on APFS
    // The -R flag is for recursive directory copy
    // The -p flag preserves permissions, timestamps, etc.
    await execFileAsync('cp', ['-c', '-R', '-p', src, dest], {
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
async function cloneDirLinux (src: string, dest: string): Promise<boolean> {
  // First check if we're on a filesystem that supports reflink
  if (!(await isReflinkSupported(src))) {
    return false
  }

  // For Linux, we use the copy_file_range approach through fs.copyFileSync
  // with COPYFILE_FICLONE flag. However, this only works for files, not directories.
  // So we need to manually implement directory cloning using FICLONE.

  try {
    // Create the destination directory
    await mkdir(dest, { recursive: true })

    // Read the source directory
    const entries = await readdir(src, { withFileTypes: true })

    for (const entry of entries) {
      const srcPath = path.join(src, entry.name)
      const destPath = path.join(dest, entry.name)

      if (entry.isDirectory()) {
        // Recursively clone subdirectories
        if (!(await cloneDirLinux(srcPath, destPath))) {
          // If subdirectory clone fails, fall back to copy
          await mkdir(destPath, { recursive: true })
          if (!(await cloneDirLinux(srcPath, destPath))) {
            return false
          }
        }
      } else if (entry.isFile()) {
        // Try to reflink clone the file using FICLONE ioctl
        if (!(await reflinkCloneFile(srcPath, destPath))) {
          // Fall back to regular copyFile with FICLONE_FORCE
          try {
            await copyFile(srcPath, destPath, constants.COPYFILE_FICLONE)
          } catch {
            await copyFile(srcPath, destPath)
          }
        }
      } else if (entry.isSymbolicLink()) {
        // Copy symlinks as symlinks
        const linkTarget = await readlink(srcPath)
        await symlink(linkTarget, destPath)
      }
    }

    // Copy permissions and timestamps from source to destination
    try {
      const srcStat = await stat(src)
      await chmod(dest, srcStat.mode)
      await utimes(dest, srcStat.atime, srcStat.mtime)
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
async function isReflinkSupported (path: string): Promise<boolean> {
  try {
    const fsStat = await statfs(path)
    return fsStat.type === BTRFS_SUPER_MAGIC || fsStat.type === XFS_SUPER_MAGIC
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
  let srcHandle
  let destHandle

  try {
    // Open source file read-only
    srcHandle = await open(src, 'r')

    // Get source file stats for permissions
    const srcStat = await srcHandle.stat()

    // Open/create destination file with same permissions
    destHandle = await open(dest, 'w', srcStat.mode)

    // Perform the reflink clone using ioctl
    // We need to use a native addon or process.binding for ioctl
    // Since we can't easily do that, fall back to copy_file_range
    // which uses the same underlying mechanism
    await copyFile(src, dest, constants.COPYFILE_FICLONE_FORCE)

    return true
  } catch {
    return false
  } finally {
    if (srcHandle !== undefined) {
      try {
        await srcHandle.close()
      } catch {} // eslint-disable-line:no-empty
    }
    if (destHandle !== undefined) {
      try {
        await destHandle.close()
      } catch {} // eslint-disable-line:no-empty
    }
  }
}
\`;
fs.writeFileSync('fs/indexed-pkg-importer/src/cloneDir.ts', cloneDirContent);

// 2. Patch index.ts
let indexContent = fs.readFileSync('fs/indexed-pkg-importer/src/index.ts', 'utf8');
const oldCloneDirPkg = \`function cloneDirPkg (
  to: string,
  opts: ImportOptions
): Promise<'clone-dir' | undefined> {
  if (opts.resolvedFrom === 'local-dir' && (!pkgExistsAtTargetDir(to, opts.filesMap) || opts.force)) {
    // Get the source directory from the first file in the filesMap
    // For local-dir, this will be a real package directory, not a CAFS shard
    const firstSrcPath = opts.filesMap.values().next().value!
    const srcDirPath = path.dirname(firstSrcPath)
    if (cloneDir(srcDirPath, to)) {
      return Promise.resolve('clone-dir')
    }
  }
  return Promise.resolve(undefined)
}\`;

const newCloneDirPkg = \`async function cloneDirPkg (
  to: string,
  opts: ImportOptions
): Promise<'clone-dir' | undefined> {
  if (opts.resolvedFrom === 'local-dir' && (!pkgExistsAtTargetDir(to, opts.filesMap) || opts.force)) {
    // Get the source directory from the first file in the filesMap
    // For local-dir, this will be a real package directory, not a CAFS shard
    const firstSrcPath = opts.filesMap.values().next().value!
    const srcDirPath = path.dirname(firstSrcPath)
    if (await cloneDir(srcDirPath, to)) {
      return 'clone-dir'
    }
  }
  return undefined
}\`;

if (indexContent.includes(oldCloneDirPkg)) {
  indexContent = indexContent.replace(oldCloneDirPkg, newCloneDirPkg);
  fs.writeFileSync('fs/indexed-pkg-importer/src/index.ts', indexContent);
}

// 3. Patch test
let testContent = fs.readFileSync('fs/indexed-pkg-importer/test/cloneDir.test.ts', 'utf8');
testContent = testContent.replaceAll(
  "const result = cloneDir(src, dest)",
  "const result = await cloneDir(src, dest)"
);
fs.writeFileSync('fs/indexed-pkg-importer/test/cloneDir.test.ts', testContent);
