import type { FileHandle } from 'node:fs/promises'
import fs from 'node:fs/promises'
import path from 'node:path'

import { isError } from '@pnpm/error'
import type { TaskKey, TaskNode } from '@pnpm/workspace.task-scheduler'
import writeFileAtomic from 'write-file-atomic'

export interface TaskId {
  project: string
  task: string
}

export interface TaskRecord extends TaskId {
  run: string
}

export interface FinishRecord {
  run: string
  finished: true
}

export class TaskRunState {
  readonly filePath: string
  private readonly publishedPath: string
  private readonly finishedPath: string
  private readonly file: FileHandle | undefined
  private readonly workspaceDir: string
  private readonly run: string
  private readonly completedTasks: Set<TaskKey>
  private pendingWrite: Promise<void> = Promise.resolve()
  private closePromise: Promise<void> | undefined
  private disabled: boolean

  constructor (
    filePath: string,
    publishedPath: string,
    finishedPath: string,
    file: FileHandle | undefined,
    workspaceDir: string,
    run: string,
    completedTasks: ReadonlySet<TaskKey>
  ) {
    this.filePath = filePath
    this.publishedPath = publishedPath
    this.finishedPath = finishedPath
    this.file = file
    this.workspaceDir = workspaceDir
    this.run = run
    this.completedTasks = new Set(completedTasks)
    this.disabled = file == null
  }

  async recordPassed (key: TaskKey, node: TaskNode): Promise<void> {
    const file = this.file
    if (this.disabled || file == null || this.completedTasks.has(key)) return
    this.completedTasks.add(key)
    const record: TaskRecord = { run: this.run, ...taskId(node, this.workspaceDir) }
    const line = `${JSON.stringify(record)}\n`
    const write = this.pendingWrite.then(async () => this.appendRecordLine(file, line))
    this.pendingWrite = write.then(ignoreOutcome, ignoreOutcome)
    let unavailable: boolean
    try {
      unavailable = await write
    } catch (err: unknown) {
      this.completedTasks.delete(key)
      throw err
    }
    if (unavailable) {
      await this.discardJournal()
    }
  }

  async finish (): Promise<void> {
    if (this.file == null || this.disabled) return
    if (this.closePromise == null) {
      await this.appendFinishRecord(this.file)
    }
    await this.close()
    if (!await this.markFinished()) return
    await unlessStateUnavailable(this.removeJournal())
  }

  async close (): Promise<void> {
    const file = this.file
    if (file == null) return
    this.closePromise ??= this.pendingWrite.then(async () => file.close())
    await this.closePromise
  }

  /**
   * Appends a task record unless the state was disabled meanwhile.
   * Resolves to `true` when the state directory turned out to be unavailable.
   */
  private async appendRecordLine (file: FileHandle, line: string): Promise<boolean> {
    if (this.disabled) return false
    try {
      await file.appendFile(line)
    } catch (err: unknown) {
      if (!isStateUnavailableError(err)) throw err
      this.disabled = true
      return true
    }
    return false
  }

  private async discardJournal (): Promise<void> {
    await this.close().catch(() => {})
    await unlinkIfExists(this.filePath).catch(() => {})
    await unlinkIfExists(this.publishedPath).catch(() => {})
  }

  private async appendFinishRecord (file: FileHandle): Promise<void> {
    const finishRecord: FinishRecord = { run: this.run, finished: true }
    const write = this.pendingWrite.then(async () => file.appendFile(`${JSON.stringify(finishRecord)}\n`))
    this.pendingWrite = write.catch(() => {})
    await unlessStateUnavailable(write)
  }

  private async markFinished (): Promise<boolean> {
    try {
      await writeFileAtomic(this.finishedPath, '', { mode: 0o600 })
    } catch (err: unknown) {
      if (isStateUnavailableError(err)) return false
      throw err
    }
    return true
  }

  private async removeJournal (): Promise<void> {
    await unlinkIfExists(this.publishedPath)
    await unlinkIfExists(this.filePath)
  }
}

function ignoreOutcome (): void {}

export function taskId (node: TaskNode, workspaceDir: string): TaskId {
  const relative = path.relative(workspaceDir, node.project)
  return {
    project: relative === '' ? '.' : relative.replaceAll(path.sep, '/'),
    task: node.taskName,
  }
}

export function compareTaskIds (left: TaskId, right: TaskId): number {
  return compareStrings(left.project, right.project) || compareStrings(left.task, right.task)
}

export function compareStrings (left: string, right: string): number {
  return left < right ? -1 : left > right ? 1 : 0
}

export async function unlinkIfExists (filePath: string): Promise<void> {
  try {
    await fs.unlink(filePath)
  } catch (err: unknown) {
    if (!hasErrorCode(err, 'ENOENT')) throw err
  }
}

/**
 * Resolves to `undefined` when the operation fails because the state
 * directory is not writable or readable, and rethrows any other failure.
 */
export async function unlessStateUnavailable<Result> (operation: Promise<Result>): Promise<Result | undefined> {
  try {
    return await operation
  } catch (err: unknown) {
    if (isStateUnavailableError(err)) return undefined
    throw err
  }
}

export function isStateUnavailableError (err: unknown): boolean {
  return isError(err) && 'code' in err &&
    (err.code === 'EACCES' || err.code === 'EPERM' || err.code === 'EROFS')
}

export function hasErrorCode (err: unknown, code: string): boolean {
  return isError(err) && 'code' in err && err.code === code
}
