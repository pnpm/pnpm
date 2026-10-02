import assert from 'node:assert'
import fs from 'node:fs/promises'
import path from 'node:path'

import { isError } from '@pnpm/error'
import { DirLock } from '@pnpm/fs.dir-lock'
import { logger } from '@pnpm/logger'
import { pathExists } from 'path-exists'

import type { DependenciesGraphNode } from './buildGraph.js'

export const NEEDS_BUILD_MARKER = '.pnpm-needs-build'
const STARTED_BUILD_MARKER_CONTENT = 'started'
const SLOT_LOCK_DIR = '.pnpm-build.lock'
const SLOT_LOCK_WAIT_MS = 10 * 60_000
// Comfortably above how long a build can take, so a live holder never has its lock stolen mid-build.
const SLOT_LOCK_ABANDONED_MS = 30 * 60_000

/**
 * Serializes builds into one global virtual store slot across processes, and
 * marks the slot as mid-build before the build writes into it. Resolves to
 * `undefined` when another install built the slot while this one waited for
 * its lock, or when another install's build of it did not finish.
 */
export async function lockSlotForBuild<NodeId extends string> (depNode: DependenciesGraphNode<NodeId>, lockfileDir: string): Promise<{ lock?: DirLock } | undefined> {
  const marker = path.join(depNode.dir, NEEDS_BUILD_MARKER)
  const awaitingBuild = await pathExists(marker)
  const lock = await lockGlobalVirtualStoreSlot(depNode.modules)
  if (await isStartedBuildMarker(marker) || (awaitingBuild && !await pathExists(marker))) {
    await lock?.release()
    return undefined
  }
  await markBuildStarted(depNode, lockfileDir)
  return { lock }
}

/**
 * Takes the lock that serializes writes into one global virtual store slot
 * across processes: builds, and re-imports of a slot that still carries its
 * `.pnpm-needs-build` marker. `slotModulesDir` is the slot's `node_modules`.
 * Resolves to `undefined` when the lock cannot be taken, and the caller then
 * writes without it: the lock avoids a race, and a race lost is better than
 * an install that refuses to run.
 */
export async function lockGlobalVirtualStoreSlot (slotModulesDir: string): Promise<DirLock | undefined> {
  const lockPath = path.join(path.dirname(slotModulesDir), SLOT_LOCK_DIR)
  try {
    return await DirLock.acquire(lockPath, { waitMs: SLOT_LOCK_WAIT_MS, abandonedMs: SLOT_LOCK_ABANDONED_MS })
  } catch (err: unknown) {
    logger.debug({ message: `Failed to lock ${lockPath}`, error: err })
    return undefined
  }
}

/**
 * Whether the slot's build started and then failed, or its process died,
 * leaving files the build may have changed. Only a re-import of the pristine
 * files, which rewrites the marker empty, makes it safe to build. A missing
 * marker resolves to `false`; any other read failure rejects, since the slot's
 * state is then unknown.
 */
async function isStartedBuildMarker (markerPath: string): Promise<boolean> {
  try {
    const content = await fs.readFile(markerPath, 'utf8')
    return content === STARTED_BUILD_MARKER_CONTENT
  } catch (err: unknown) {
    if (isError(err) && 'code' in err && err.code === 'ENOENT') return false
    throw err
  }
}

/**
 * A successful build removes the marker. A failed patch or build script, or a
 * process that dies mid-build, leaves it in place instead of the slot being
 * removed, because other projects may already link the shared slot. The next
 * install that reaches it then re-imports its pristine files and builds again.
 */
async function markBuildStarted<NodeId extends string> (depNode: DependenciesGraphNode<NodeId>, lockfileDir: string): Promise<void> {
  try {
    await fs.writeFile(path.join(depNode.dir, NEEDS_BUILD_MARKER), STARTED_BUILD_MARKER_CONTENT)
  } catch (err: unknown) {
    assert(isError(err))
    if ('code' in err && err.code === 'ENOENT') return
    logger.warn({
      error: err,
      message: `Failed to mark ${depNode.dir} as mid-build`,
      prefix: lockfileDir,
    })
  }
}
