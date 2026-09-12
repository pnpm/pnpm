import crypto from 'node:crypto'
import fs, { type FileHandle } from 'node:fs/promises'
import path from 'node:path'
import util from 'node:util'

import { createHexHash } from '@pnpm/crypto.hash'
import { PnpmError } from '@pnpm/error'
import type { TaskGraph, TaskKey, TaskNode } from '@pnpm/workspace.task-scheduler'
import writeFileAtomic from 'write-file-atomic'

const STATE_VERSION = 1
const STATE_DIR = '.pnpm-task-run-state-v1'
const LATEST_STATE_FILE = 'latest.json'
const PUBLISHED_SUFFIX = '.published'
const FINISHED_SUFFIX = '.finished'
const START_LOCK_DIR = 'start.lock'
const LOCK_OWNER_FILE = 'owner'
const LOCK_POLL_INTERVAL_MS = 50
const LOCK_WAIT_MS = 2_000
const LOCK_ABANDONED_MS = 30_000
const RUN_GENERATION_LENGTH = 12
const RUN_ID = /^[0-9a-f]{12}-[0-9a-f-]{1,115}$/

interface TaskId {
  project: string
  task: string
}

interface TaskIdentity extends TaskId {
  scripts: Array<{ name: string, commands: string[] }>
  requested: boolean
  dependencies: TaskId[]
}

interface InvocationIdentity {
  command: 'run' | 'exec'
  params: string[]
  settings: string[]
  tasks: TaskIdentity[]
}

interface StateHeader {
  version: number
  invocation: string
  run: string
}

interface TaskRecord extends TaskId {
  run: string
}

interface FinishRecord {
  run: string
  finished: true
}

export interface TaskRunStateContextOptions {
  command: InvocationIdentity['command']
  params: string[]
  settings?: string[]
  graph: TaskGraph
  workspaceDir: string
  scriptCommands: (node: TaskNode, script: string) => string[]
}

export interface TaskRunExecutionSettings {
  extraBinPaths?: string[]
  extraEnv?: Record<string, string | undefined>
  modulesDir?: string
  nodeExperimentalPackageMap?: boolean
  nodeOptions?: string
  userAgent?: string
}

export function taskRunExecutionSettings (opts: TaskRunExecutionSettings): string[] {
  const extraEnv = Object.entries(opts.extraEnv ?? {})
    .sort(([left], [right]) => compareStrings(left, right))
    .map(([key, value]) => [key, value ?? null])
  return [
    `extra-bin-paths=${JSON.stringify(opts.extraBinPaths ?? [])}`,
    `extra-env=${JSON.stringify(extraEnv)}`,
    `modules-dir=${opts.modulesDir ?? 'node_modules'}`,
    `node-experimental-package-map=${Boolean(opts.nodeExperimentalPackageMap)}`,
    `node-options=${opts.nodeOptions ?? ''}`,
    `user-agent=${opts.userAgent ?? ''}`,
  ]
}

export class TaskRunStateContext {
  readonly invocation: string
  readonly latestStatePath: string
  private readonly opts: TaskRunStateContextOptions
  private readonly nodeModulesDir: string
  private readonly stateDir: string
  private readonly keysById = new Map<string, TaskKey>()

  constructor (opts: TaskRunStateContextOptions) {
    this.opts = opts
    const identity: InvocationIdentity = {
      command: opts.command,
      params: opts.params,
      settings: [...(opts.settings ?? [])].sort(compareStrings),
      tasks: [...opts.graph].map(([key, node]) => {
        const id = taskId(node, opts.workspaceDir)
        this.keysById.set(taskIdKey(id), key)
        return {
          ...id,
          scripts: node.scripts
            .map((name) => ({ name, commands: opts.scriptCommands(node, name) }))
            .sort(compareScripts),
          requested: node.requested,
          dependencies: node.dependencies
            .map((dependency) => taskId(opts.graph.get(dependency)!, opts.workspaceDir))
            .sort(compareTaskIds),
        }
      }).sort(compareTaskIds),
    }
    this.invocation = createHexHash(JSON.stringify(identity))
    this.nodeModulesDir = path.join(opts.workspaceDir, 'node_modules')
    this.stateDir = path.join(this.nodeModulesDir, STATE_DIR)
    this.latestStatePath = path.join(this.stateDir, LATEST_STATE_FILE)
  }

  async readCompletedTasks (): Promise<Set<TaskKey> | undefined> {
    try {
      if (!await this.validateStateDirectory(false)) return undefined
    } catch (err: unknown) {
      if (isStateUnavailableError(err)) return undefined
      throw err
    }
    let latest: StateHeader
    try {
      latest = JSON.parse(await fs.readFile(this.latestStatePath, 'utf8')) as StateHeader
    } catch (err: unknown) {
      if (util.types.isNativeError(err) && 'code' in err && err.code !== 'ENOENT' && !isStateUnavailableError(err)) throw err
      return undefined
    }
    if (latest.version !== STATE_VERSION || latest.invocation !== this.invocation || !RUN_ID.test(latest.run)) return undefined
    let state: { run: string, finished: boolean }
    try {
      state = await this.newestState(latest.run)
    } catch (err: unknown) {
      if (isStateUnavailableError(err)) return undefined
      throw err
    }
    if (state.finished) return undefined
    const filePath = this.journalPath(state.run)
    let contents: string
    try {
      contents = await fs.readFile(filePath, 'utf8')
    } catch (err: unknown) {
      if (util.types.isNativeError(err) && 'code' in err && (err.code === 'ENOENT' || isStateUnavailableError(err))) return undefined
      throw err
    }
    // A record is committed by its newline; a process killed during append
    // can leave only the final record torn.
    const lines = contents.split('\n')
    lines.pop()
    if (lines.length === 0) return undefined
    let header: StateHeader
    try {
      header = JSON.parse(lines[0]) as StateHeader
    } catch {
      return undefined
    }
    if (header.version !== STATE_VERSION || header.invocation !== this.invocation || header.run !== state.run) return undefined
    const completed = new Set<TaskKey>()
    for (const line of lines.slice(1)) {
      let record: TaskRecord | FinishRecord
      try {
        record = JSON.parse(line) as TaskRecord | FinishRecord
      } catch {
        return undefined
      }
      if (record.run !== header.run) continue
      if (isFinishRecord(record)) return undefined
      const key = this.keysById.get(taskIdKey(record))
      if (key == null) return undefined
      completed.add(key)
    }
    return completed
  }

  async start (completedTasks: ReadonlySet<TaskKey>): Promise<TaskRunState> {
    let run = createRunId(Date.now())
    let filePath = this.journalPath(run)
    let file: FileHandle | undefined
    let journalCreated = false
    let lock: StateStartLock | undefined
    try {
      await this.validateStateDirectory(true)
      lock = await StateStartLock.acquire(path.join(this.stateDir, START_LOCK_DIR))
      if (lock == null) {
        return new TaskRunState(filePath, this.publishedPath(run), this.finishedPath(run), undefined, this.opts.workspaceDir, run, completedTasks)
      }
      run = await this.nextRunId()
      filePath = this.journalPath(run)
      const header: StateHeader = { version: STATE_VERSION, invocation: this.invocation, run }
      const completed = [...completedTasks]
        .map((key): TaskRecord => ({ run, ...taskId(this.opts.graph.get(key)!, this.opts.workspaceDir) }))
        .sort(compareTaskIds)
      const contents = [header, ...completed].map((record) => JSON.stringify(record)).join('\n') + '\n'
      await writeFileAtomic(filePath, contents, { mode: 0o600 })
      journalCreated = true
      file = await fs.open(filePath, 'a')
      if (!await lock.isOwner()) {
        await file.close()
        file = undefined
        await unlinkIfExists(filePath)
        journalCreated = false
        return new TaskRunState(filePath, this.publishedPath(run), this.finishedPath(run), undefined, this.opts.workspaceDir, run, completedTasks)
      }
      await writeFileAtomic(this.latestStatePath, JSON.stringify(header), { mode: 0o600 })
      await writeFileAtomic(this.publishedPath(run), '', { mode: 0o600 })
      await this.cleanupOlderFinishedState(run).catch(() => {})
    } catch (err: unknown) {
      await file?.close().catch(() => {})
      if (journalCreated) await unlinkIfExists(filePath).catch(() => {})
      await unlinkIfExists(this.publishedPath(run)).catch(() => {})
      if (isStateUnavailableError(err)) {
        return new TaskRunState(filePath, this.publishedPath(run), this.finishedPath(run), undefined, this.opts.workspaceDir, run, completedTasks)
      }
      throw err
    } finally {
      await lock?.release()
    }
    return new TaskRunState(filePath, this.publishedPath(run), this.finishedPath(run), file, this.opts.workspaceDir, run, completedTasks)
  }

  private journalPath (run: string): string {
    return path.join(this.stateDir, `${this.invocation}.${run}.jsonl`)
  }

  private publishedPath (run: string): string {
    return path.join(this.stateDir, `${this.invocation}.${run}${PUBLISHED_SUFFIX}`)
  }

  private finishedPath (run: string): string {
    return path.join(this.stateDir, `${this.invocation}.${run}${FINISHED_SUFFIX}`)
  }

  private async newestState (latestRun: string): Promise<{ run: string, finished: boolean }> {
    let newestRun = latestRun
    const prefix = `${this.invocation}.`
    const names = new Set(await fs.readdir(this.stateDir))
    let finished = names.has(`${prefix}${latestRun}${FINISHED_SUFFIX}`)
    for (const name of names) {
      if (!name.startsWith(prefix)) continue
      let run: string
      let candidateFinished: boolean
      if (name.endsWith(FINISHED_SUFFIX)) {
        run = name.slice(prefix.length, -FINISHED_SUFFIX.length)
        candidateFinished = true
      } else if (name.endsWith('.jsonl')) {
        run = name.slice(prefix.length, -'.jsonl'.length)
        if (!names.has(`${prefix}${run}${PUBLISHED_SUFFIX}`)) continue
        candidateFinished = false
      } else {
        continue
      }
      if (!RUN_ID.test(run)) continue
      if (runGeneration(run) > runGeneration(newestRun)) {
        newestRun = run
        finished = candidateFinished
      } else if (run === newestRun && candidateFinished) {
        finished = true
      }
    }
    return { run: newestRun, finished }
  }

  private async nextRunId (): Promise<string> {
    let newestGeneration = Date.now().toString(16).padStart(RUN_GENERATION_LENGTH, '0')
    try {
      const latest = JSON.parse(await fs.readFile(this.latestStatePath, 'utf8')) as StateHeader
      if (latest.invocation === this.invocation && RUN_ID.test(latest.run)) {
        newestGeneration = maxString(newestGeneration, runGeneration(latest.run))
      }
    } catch (err: unknown) {
      if (util.types.isNativeError(err) && 'code' in err && err.code !== 'ENOENT') throw err
    }
    const prefix = `${this.invocation}.`
    for (const name of await fs.readdir(this.stateDir)) {
      if (!name.startsWith(prefix)) continue
      const suffix = name.endsWith('.jsonl') ? '.jsonl' : name.endsWith(FINISHED_SUFFIX) ? FINISHED_SUFFIX : undefined
      if (suffix == null) continue
      const run = name.slice(prefix.length, -suffix.length)
      if (RUN_ID.test(run)) newestGeneration = maxString(newestGeneration, runGeneration(run))
    }
    return createRunId(Number.parseInt(newestGeneration, 16) + 1)
  }

  private async cleanupOlderFinishedState (run: string): Promise<void> {
    const prefix = `${this.invocation}.`
    const generation = runGeneration(run)
    const removals: Array<Promise<void>> = []
    for (const name of await fs.readdir(this.stateDir)) {
      if (!name.startsWith(prefix)) continue
      if (!name.endsWith(FINISHED_SUFFIX)) continue
      const olderRun = name.slice(prefix.length, -FINISHED_SUFFIX.length)
      if (!RUN_ID.test(olderRun) || runGeneration(olderRun) >= generation) continue
      removals.push(unlinkIfExists(path.join(this.stateDir, name)).catch(() => {}))
    }
    await Promise.all(removals)
  }

  private async validateStateDirectory (create: boolean): Promise<boolean> {
    if (!await validateRealDirectory(this.nodeModulesDir, create)) return false
    return validateRealDirectory(this.stateDir, create)
  }
}

function createRunId (generation: number): string {
  return `${generation.toString(16).padStart(RUN_GENERATION_LENGTH, '0')}-${crypto.randomUUID()}`
}

function runGeneration (run: string): string {
  return run.slice(0, RUN_GENERATION_LENGTH)
}

function maxString (left: string, right: string): string {
  return left > right ? left : right
}

function isFinishRecord (record: TaskRecord | FinishRecord): record is FinishRecord {
  return 'finished' in record && record.finished
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
    let unavailable = false
    const write = this.pendingWrite.then(async () => {
      if (this.disabled) return
      try {
        await file.appendFile(line)
      } catch (err: unknown) {
        if (!isStateUnavailableError(err)) throw err
        this.disabled = true
        unavailable = true
      }
    })
    this.pendingWrite = write.catch(() => {})
    try {
      await write
    } catch (err: unknown) {
      this.completedTasks.delete(key)
      throw err
    }
    if (unavailable) {
      await this.close().catch(() => {})
      await unlinkIfExists(this.filePath).catch(() => {})
      await unlinkIfExists(this.publishedPath).catch(() => {})
    }
  }

  async finish (): Promise<void> {
    if (this.file == null || this.disabled) return
    if (this.closePromise == null) {
      const finishRecord: FinishRecord = { run: this.run, finished: true }
      const write = this.pendingWrite.then(async () => this.file!.appendFile(`${JSON.stringify(finishRecord)}\n`))
      this.pendingWrite = write.catch(() => {})
      try {
        await write
      } catch (err: unknown) {
        if (!isStateUnavailableError(err)) throw err
      }
    }
    await this.close()
    try {
      await writeFileAtomic(this.finishedPath, '', { mode: 0o600 })
    } catch (err: unknown) {
      if (isStateUnavailableError(err)) return
      throw err
    }
    try {
      await unlinkIfExists(this.publishedPath)
      await unlinkIfExists(this.filePath)
    } catch (err: unknown) {
      if (!isStateUnavailableError(err)) throw err
    }
  }

  async close (): Promise<void> {
    const file = this.file
    if (file == null) return
    this.closePromise ??= this.pendingWrite.then(async () => file.close())
    await this.closePromise
  }
}

class StateStartLock {
  private readonly lockPath: string
  private readonly token: string

  private constructor (
    lockPath: string,
    token: string
  ) {
    this.lockPath = lockPath
    this.token = token
  }

  static async acquire (lockPath: string): Promise<StateStartLock | undefined> {
    return StateStartLock.acquireUntil(lockPath, Date.now() + LOCK_WAIT_MS)
  }

  private static async acquireUntil (lockPath: string, deadline: number): Promise<StateStartLock | undefined> {
    try {
      await fs.mkdir(lockPath)
      const token = `${process.pid}-${Date.now()}-${crypto.randomUUID()}`
      try {
        await fs.writeFile(path.join(lockPath, LOCK_OWNER_FILE), token, { mode: 0o600 })
      } catch (err: unknown) {
        await fs.rm(lockPath, { force: true, recursive: true }).catch(() => {})
        throw err
      }
      return new StateStartLock(lockPath, token)
    } catch (err: unknown) {
      if (!(util.types.isNativeError(err) && 'code' in err && err.code === 'EEXIST')) throw err
    }
    const stats = await fs.lstat(lockPath).catch(() => undefined)
    if (stats == null || stats.isSymbolicLink() || !stats.isDirectory()) return undefined
    if (Date.now() - stats.mtimeMs > LOCK_ABANDONED_MS) {
      const removed = await fs.rm(lockPath, { force: true, recursive: true }).then(() => true, () => false)
      if (removed) return StateStartLock.acquireUntil(lockPath, deadline)
    }
    if (Date.now() >= deadline) return undefined
    await new Promise(resolve => setTimeout(resolve, LOCK_POLL_INTERVAL_MS))
    return StateStartLock.acquireUntil(lockPath, deadline)
  }

  async isOwner (): Promise<boolean> {
    try {
      return await fs.readFile(path.join(this.lockPath, LOCK_OWNER_FILE), 'utf8') === this.token
    } catch (err: unknown) {
      if (util.types.isNativeError(err) && 'code' in err && err.code === 'ENOENT') return false
      throw err
    }
  }

  async release (): Promise<void> {
    if (!await this.isOwner().catch(() => false)) return
    await fs.rm(this.lockPath, { force: true, recursive: true }).catch(() => {})
  }
}

async function validateRealDirectory (dir: string, create: boolean): Promise<boolean> {
  let stats
  try {
    stats = await fs.lstat(dir)
  } catch (err: unknown) {
    if (!(util.types.isNativeError(err) && 'code' in err && err.code === 'ENOENT')) throw err
    if (!create) return false
    try {
      await fs.mkdir(dir)
    } catch (mkdirErr: unknown) {
      if (!(util.types.isNativeError(mkdirErr) && 'code' in mkdirErr && mkdirErr.code === 'EEXIST')) throw mkdirErr
    }
    stats = await fs.lstat(dir)
  }
  if (stats.isSymbolicLink() || !stats.isDirectory()) {
    throw new PnpmError('UNSAFE_TASK_RUN_STATE_PATH', `Refusing to use task run state directory at "${dir}" because it is a symbolic link or not a directory`)
  }
  return true
}

function taskId (node: TaskNode, workspaceDir: string): TaskId {
  const relative = path.relative(workspaceDir, node.project)
  return {
    project: relative === '' ? '.' : relative.replaceAll(path.sep, '/'),
    task: node.taskName,
  }
}

function taskIdKey (id: TaskId): string {
  return `${id.project}\0${id.task}`
}

function compareTaskIds (left: TaskId, right: TaskId): number {
  return compareStrings(left.project, right.project) || compareStrings(left.task, right.task)
}

function compareScripts (left: { name: string }, right: { name: string }): number {
  return compareStrings(left.name, right.name)
}

function compareStrings (left: string, right: string): number {
  return left < right ? -1 : left > right ? 1 : 0
}

async function unlinkIfExists (filePath: string): Promise<void> {
  try {
    await fs.unlink(filePath)
  } catch (err: unknown) {
    if (!(util.types.isNativeError(err) && 'code' in err && err.code === 'ENOENT')) throw err
  }
}

function isStateUnavailableError (err: unknown): boolean {
  return util.types.isNativeError(err) && 'code' in err &&
    (err.code === 'EACCES' || err.code === 'EPERM' || err.code === 'EROFS')
}
