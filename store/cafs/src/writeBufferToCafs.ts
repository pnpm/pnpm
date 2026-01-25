import fs from 'fs'
import path from 'path'
import workerThreads from 'worker_threads'
import util from 'util'
import renameOverwrite from 'rename-overwrite'
import { type Integrity, verifyFileIntegrity } from './checkPkgFilesIntegrity.js'
import { writeFile } from './writeFile.js'

export function writeBufferToCafs (
  locker: Map<string, number>,
  storeDir: string,
  buffer: Buffer,
  fileDest: string,
  mode: number | undefined,
  integrity: Integrity
): { checkedAt: number, filePath: string } {
  fileDest = path.join(storeDir, fileDest)
  if (locker.has(fileDest)) {
    return {
      checkedAt: locker.get(fileDest)!,
      filePath: fileDest,
    }
  }
  // This part is a bit redundant.
  // When a file is already used by another package,
  // we probably have validated its content already.
  // However, there is no way to find which package index file references
  // the given file. So we should revalidate the content of the file again.
  if (existsSame(fileDest, integrity)) {
    return {
      checkedAt: Date.now(),
      filePath: fileDest,
    }
  }

  // This might be too cautious.
  // The write is atomic, so in case pnpm crashes, no broken file
  // will be added to the store.
  // It might be a redundant step though, as we verify the contents of the
  // files before linking
  //
  // If we don't allow --no-verify-store-integrity then we probably can write
  // to the final file directly.
  const temp = pathTemp(fileDest)
  writeFile(temp, buffer, mode)
  // Unfortunately, "birth time" (time of file creation) is available not on all filesystems.
  // We log the creation time ourselves and save it in the package index file.
  // Having this information allows us to skip content checks for files that were not modified since "birth time".
  const birthtimeMs = Date.now()
  optimisticRenameOverwrite(temp, fileDest)
  locker.set(fileDest, birthtimeMs)
  return {
    checkedAt: birthtimeMs,
    filePath: fileDest,
  }
}

export function optimisticRenameOverwrite (temp: string, fileDest: string): void {
  try {
    renameOverwrite.sync(temp, fileDest)
  } catch (err: unknown) {
    if (!(util.types.isNativeError(err) && 'code' in err && err.code === 'ENOENT') || !fs.existsSync(fileDest)) throw err
    // The temporary file path is created by appending the process ID to the target file name.
    // This is done to avoid lots of random crypto number generations.
    //   PR with related performance optimization: https://github.com/pnpm/pnpm/pull/6817
    //
    // Probably the only scenario in which the temp directory will disappear
    // before being renamed is when two containers use the same mounted directory
    // for their content-addressable store. In this case there's a chance that the process ID
    // will be the same in both containers.
    //
    // As a workaround, if the temp file doesn't exist but the target file does,
    // we just ignore the issue and assume that the target file is correct.
  }
}

/**
 * Creates a unique temporary file path by appending both process ID and worker thread ID
 * to the original filename.
 *
 * The process ID prevents conflicts between different processes, while the worker thread ID
 * prevents race conditions between threads in the same process.
 *
 * If a process fails, its temporary file may remain. When the process is rerun, it will
 * safely overwrite any existing temporary file with the same name.
 *
 * @param file - The original file path
 * @returns A temporary file path in the format: {basename}{pid}{threadId}
 */
export function pathTemp (file: string): string {
  const basename = removeSuffix(path.basename(file))
  return path.join(path.dirname(file), `${basename}${process.pid}${workerThreads.threadId}`)
}

function removeSuffix (filePath: string): string {
  const dashPosition = filePath.indexOf('-')
  if (dashPosition === -1) return filePath
  const withoutSuffix = filePath.substring(0, dashPosition)
  if (filePath.substring(dashPosition) === '-exec') {
    return `${withoutSuffix}x`
  }
  return withoutSuffix
}

function existsSame (filename: string, integrity: Integrity): boolean {
  const existingFile = fs.statSync(filename, { throwIfNoEntry: false })
  if (!existingFile) return false
  return verifyFileIntegrity(filename, integrity)
}
