import crypto from 'node:crypto'
import type { Stats } from 'node:fs'
import fs, { type FileHandle } from 'node:fs/promises'
import path from 'node:path'

import { createHexHash } from '@pnpm/crypto.hash'
import { isError, PnpmError } from '@pnpm/error'
import { DirLock } from '@pnpm/fs.dir-lock'
import type { TaskGraph, TaskKey, TaskNode } from '@pnpm/workspace.task-scheduler'
import writeFileAtomic from 'write-file-atomic'

import {
  compareStrings,
  compareTaskIds,
  type FinishRecord,
  hasErrorCode,
  isStateUnavailableError,
  type TaskId,
  taskId,
  type TaskRecord,
  TaskRunState,
  unlessStateUnavailable,
  unlinkIfExists,
} from './taskRunStateJournal.js'

export type { TaskRunState }

const STATE_VERSION = 1
const STATE_DIR = '.pnpm-task-run-state-v1'
const LATEST_STATE_FILE = 'latest.json'
const PUBLISHED_SUFFIX = '.published'
const FINISHED_SUFFIX = '.finished'
const START_LOCK_DIR = 'start.lock'
const LOCK_WAIT_MS = 2_000
const LOCK_ABANDONED_MS = 30_000
const RUN_GENERATION_LENGTH = 12
const RUN_ID = /^[0-9a-f]{12}-[0-9a-f-]{1,115}$/

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
    if (!await unlessStateUnavailable(this.validateStateDirectory(false))) return undefined
    const latest = await this.readLatestState()
    if (latest == null || !this.isCurrentInvocation(latest) || !RUN_ID.test(latest.run)) return undefined
    const state = await unlessStateUnavailable(this.newestState(latest.run))
    if (state == null || state.finished) return undefined
    const lines = await readJournalLines(this.journalPath(state.run))
    if (lines == null || lines.length === 0) return undefined
    const header = parseJson<StateHeader>(lines[0])
    if (header == null || !this.isCurrentInvocation(header) || header.run !== state.run) return undefined
    return this.collectCompletedTasks(lines.slice(1), header.run)
  }

  async start (completedTasks: ReadonlySet<TaskKey>): Promise<TaskRunState> {
    let run = createRunId(Date.now())
    let file: FileHandle | undefined
    let journalCreated = false
    let lock: DirLock | undefined
    try {
      await this.validateStateDirectory(true)
      lock = await DirLock.acquire(path.join(this.stateDir, START_LOCK_DIR), { waitMs: LOCK_WAIT_MS, abandonedMs: LOCK_ABANDONED_MS })
      if (lock == null) {
        return this.createState(run, undefined, completedTasks)
      }
      run = await this.nextRunId()
      const header = await this.writeJournal(run, completedTasks)
      journalCreated = true
      file = await fs.open(this.journalPath(run), 'a')
      if (!await lock.isOwner()) {
        await file.close()
        file = undefined
        await unlinkIfExists(this.journalPath(run))
        journalCreated = false
        return this.createState(run, undefined, completedTasks)
      }
      await this.publishRun(header)
    } catch (err: unknown) {
      await file?.close().catch(() => {})
      if (journalCreated) await unlinkIfExists(this.journalPath(run)).catch(() => {})
      await unlinkIfExists(this.publishedPath(run)).catch(() => {})
      if (isStateUnavailableError(err)) {
        return this.createState(run, undefined, completedTasks)
      }
      throw err
    } finally {
      await lock?.release()
    }
    return this.createState(run, file, completedTasks)
  }

  private createState (run: string, file: FileHandle | undefined, completedTasks: ReadonlySet<TaskKey>): TaskRunState {
    return new TaskRunState(this.journalPath(run), this.publishedPath(run), this.finishedPath(run), file, this.opts.workspaceDir, run, completedTasks)
  }

  private async writeJournal (run: string, completedTasks: ReadonlySet<TaskKey>): Promise<StateHeader> {
    const header: StateHeader = { version: STATE_VERSION, invocation: this.invocation, run }
    const completed = [...completedTasks]
      .map((key): TaskRecord => ({ run, ...taskId(this.opts.graph.get(key)!, this.opts.workspaceDir) }))
      .sort(compareTaskIds)
    const contents = [header, ...completed].map((record) => JSON.stringify(record)).join('\n') + '\n'
    await writeFileAtomic(this.journalPath(run), contents, { mode: 0o600 })
    return header
  }

  private async publishRun (header: StateHeader): Promise<void> {
    await writeFileAtomic(this.latestStatePath, JSON.stringify(header), { mode: 0o600 })
    await writeFileAtomic(this.publishedPath(header.run), '', { mode: 0o600 })
    await this.cleanupOlderFinishedState(header.run).catch(() => {})
  }

  private async readLatestState (): Promise<StateHeader | undefined> {
    try {
      return JSON.parse(await fs.readFile(this.latestStatePath, 'utf8')) as StateHeader
    } catch (err: unknown) {
      if (isError(err) && 'code' in err && err.code !== 'ENOENT' && !isStateUnavailableError(err)) throw err
      return undefined
    }
  }

  private isCurrentInvocation (header: StateHeader): boolean {
    return header.version === STATE_VERSION && header.invocation === this.invocation
  }

  private collectCompletedTasks (recordLines: string[], run: string): Set<TaskKey> | undefined {
    const completed = new Set<TaskKey>()
    for (const line of recordLines) {
      const record = parseJson<TaskRecord | FinishRecord>(line)
      if (record == null) return undefined
      if (record.run !== run) continue
      if (isFinishRecord(record)) return undefined
      const key = this.keysById.get(taskIdKey(record))
      if (key == null) return undefined
      completed.add(key)
    }
    return completed
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
      const candidate = parsePublishedRun(name, prefix, names)
      if (candidate == null || !RUN_ID.test(candidate.run)) continue
      if (runGeneration(candidate.run) > runGeneration(newestRun)) {
        newestRun = candidate.run
        finished = candidate.finished
      } else if (candidate.run === newestRun && candidate.finished) {
        finished = true
      }
    }
    return { run: newestRun, finished }
  }

  private async nextRunId (): Promise<string> {
    let newestGeneration = Date.now().toString(16).padStart(RUN_GENERATION_LENGTH, '0')
    const latestRun = await this.readLatestRunOfInvocation()
    if (latestRun != null) {
      newestGeneration = maxString(newestGeneration, runGeneration(latestRun))
    }
    const prefix = `${this.invocation}.`
    for (const name of await fs.readdir(this.stateDir)) {
      const run = parseStateFileRun(name, prefix)
      if (run != null && RUN_ID.test(run)) newestGeneration = maxString(newestGeneration, runGeneration(run))
    }
    return createRunId(Number.parseInt(newestGeneration, 16) + 1)
  }

  private async readLatestRunOfInvocation (): Promise<string | undefined> {
    try {
      const latest = JSON.parse(await fs.readFile(this.latestStatePath, 'utf8')) as StateHeader
      if (latest.invocation === this.invocation && RUN_ID.test(latest.run)) return latest.run
    } catch (err: unknown) {
      if (isError(err) && 'code' in err && err.code !== 'ENOENT') throw err
    }
    return undefined
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

/**
 * Returns the run of a journal that was published or of a finished run.
 * A journal that was never published belongs to a run that has not started.
 */
function parsePublishedRun (name: string, prefix: string, names: Set<string>): { run: string, finished: boolean } | undefined {
  if (!name.startsWith(prefix)) return undefined
  if (name.endsWith(FINISHED_SUFFIX)) {
    return { run: name.slice(prefix.length, -FINISHED_SUFFIX.length), finished: true }
  }
  if (!name.endsWith('.jsonl')) return undefined
  const run = name.slice(prefix.length, -'.jsonl'.length)
  if (!names.has(`${prefix}${run}${PUBLISHED_SUFFIX}`)) return undefined
  return { run, finished: false }
}

function parseStateFileRun (name: string, prefix: string): string | undefined {
  if (!name.startsWith(prefix)) return undefined
  const suffix = name.endsWith('.jsonl') ? '.jsonl' : name.endsWith(FINISHED_SUFFIX) ? FINISHED_SUFFIX : undefined
  if (suffix == null) return undefined
  return name.slice(prefix.length, -suffix.length)
}

async function readJournalLines (filePath: string): Promise<string[] | undefined> {
  let contents: string
  try {
    contents = await fs.readFile(filePath, 'utf8')
  } catch (err: unknown) {
    if (isError(err) && 'code' in err && (err.code === 'ENOENT' || isStateUnavailableError(err))) return undefined
    throw err
  }
  // A record is committed by its newline; a process killed during append
  // can leave only the final record torn.
  const lines = contents.split('\n')
  lines.pop()
  return lines
}

function parseJson<Value> (text: string): Value | undefined {
  try {
    return JSON.parse(text) as Value
  } catch {
    return undefined
  }
}

async function validateRealDirectory (dir: string, create: boolean): Promise<boolean> {
  const stats = await lstatCreatingMissingDirectory(dir, create)
  if (stats == null) return false
  if (stats.isSymbolicLink() || !stats.isDirectory()) {
    throw new PnpmError('UNSAFE_TASK_RUN_STATE_PATH', `Refusing to use task run state directory at "${dir}" because it is a symbolic link or not a directory`)
  }
  return true
}

async function lstatCreatingMissingDirectory (dir: string, create: boolean): Promise<Stats | undefined> {
  try {
    return await fs.lstat(dir)
  } catch (err: unknown) {
    if (!hasErrorCode(err, 'ENOENT')) throw err
    if (!create) return undefined
  }
  try {
    await fs.mkdir(dir)
  } catch (mkdirErr: unknown) {
    if (!hasErrorCode(mkdirErr, 'EEXIST')) throw mkdirErr
  }
  return fs.lstat(dir)
}

function taskIdKey (id: TaskId): string {
  return `${id.project}\0${id.task}`
}

function compareScripts (left: { name: string }, right: { name: string }): number {
  return compareStrings(left.name, right.name)
}
