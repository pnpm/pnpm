import fs from 'node:fs'
import path from 'node:path'

import { isError, PnpmError } from '@pnpm/error'

// A standalone pnpm executable copied into a directory on PATH (pnpm v10
// installed it into pnpmHomeDir). Windows prefers pnpm.exe over the pnpm.cmd
// shim that self-update links, so a leftover one keeps running the old version.
const STANDALONE_EXECUTABLE = 'pnpm.exe'
const RETIRED_EXECUTABLE_SUFFIX = '.retired'

export interface RetiredExecutable {
  executable: string
  retired: string
}

// Windows refuses to delete a running executable but lets it be renamed, so
// the executable is renamed out of the way of the shims. A native pnpm v12
// shim named pnpm carries a sidecar and is not a leftover.
export async function retireStandaloneExecutable (dir: string): Promise<RetiredExecutable | undefined> {
  await removeRetiredExecutables(dir)
  const executable = path.join(dir, STANDALONE_EXECUTABLE)
  if (!fs.existsSync(executable) || fs.existsSync(path.join(dir, '.pnpm-shim-v1-pnpm-target'))) {
    return undefined
  }
  const retired = path.join(dir, `.${STANDALONE_EXECUTABLE}.${process.pid}${RETIRED_EXECUTABLE_SUFFIX}`)
  await fs.promises.rename(executable, retired)
  return { executable, retired }
}

// The retired executable is put back when linking fails, so the directory
// keeps a working pnpm. One that is still running stays until a later
// self-update removes it.
export async function linkReplacingRetiredExecutable (retired: RetiredExecutable | undefined, link: () => Promise<unknown>): Promise<void> {
  try {
    await link()
  } catch (linkError: unknown) {
    if (retired != null) {
      await restoreRetiredExecutable(retired, linkError)
    }
    throw linkError
  }
  if (retired != null) {
    await removeRetiredExecutable(retired.retired)
  }
}

async function restoreRetiredExecutable (retired: RetiredExecutable, linkError: unknown): Promise<void> {
  try {
    await fs.promises.rename(retired.retired, retired.executable)
  } catch (restoreError: unknown) {
    const reason = isError(restoreError) ? restoreError.message : String(restoreError)
    throw new PnpmError('SELF_UPDATE_RESTORE_FAILED', `Linking the updated pnpm failed, and ${retired.executable} could not be restored from ${retired.retired}: ${reason}`, { cause: linkError })
  }
}

async function removeRetiredExecutables (dir: string): Promise<void> {
  let fileNames: string[]
  try {
    fileNames = await fs.promises.readdir(dir)
  } catch (err: unknown) {
    if (isError(err) && 'code' in err && err.code === 'ENOENT') return
    throw err
  }
  await Promise.all(fileNames
    .filter(isRetiredByEndedProcess)
    .map((fileName) => removeRetiredExecutable(path.join(dir, fileName))))
}

// A retired executable is named after the process that retired it. While
// that process runs, it may still need the file to restore pnpm.exe.
function isRetiredByEndedProcess (fileName: string): boolean {
  const prefix = `.${STANDALONE_EXECUTABLE}.`
  if (!fileName.startsWith(prefix) || !fileName.endsWith(RETIRED_EXECUTABLE_SUFFIX)) return false
  const pid = Number(fileName.slice(prefix.length, -RETIRED_EXECUTABLE_SUFFIX.length))
  return !Number.isInteger(pid) || !isProcessRunning(pid)
}

function isProcessRunning (pid: number): boolean {
  try {
    process.kill(pid, 0)
    return true
  } catch (err: unknown) {
    return isError(err) && 'code' in err && err.code === 'EPERM'
  }
}

async function removeRetiredExecutable (retired: string): Promise<void> {
  try {
    await fs.promises.rm(retired, { force: true })
  } catch (err: unknown) {
    if (isError(err) && 'code' in err && (err.code === 'EPERM' || err.code === 'EBUSY')) return
    throw err
  }
}
