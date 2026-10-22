import fs from 'node:fs'
import path from 'node:path'

interface RootSnapshot {
  mtimeMs: number
  entries: string
}

interface RootState {
  connections: number
  snapshot?: RootSnapshot
}

const roots = new Map<string, RootState>()

/**
 * SQLite creates the `index.db-wal` and `index.db-shm` sidecars when a
 * connection opens and removes them when the last one closes. Each step bumps
 * the mtime of the store directory, so an install that adds nothing to the
 * store would still make CI caches keyed on mtimes (such as GitLab's)
 * re-upload the store. Call this before opening a connection and
 * {@link releaseRootMtime} after closing it. The last release in this thread
 * restores the mtime taken by the first acquire if the directory's entries
 * are unchanged.
 */
export function acquireRootMtime (storeDir: string): string {
  const root = realpathOrResolve(storeDir)
  let state = roots.get(root)
  if (state == null) {
    state = { connections: 0, snapshot: takeSnapshot(root) }
    roots.set(root, state)
  }
  state.connections++
  return root
}

/** Release the root returned by {@link acquireRootMtime}. */
export function releaseRootMtime (root: string): void {
  const state = roots.get(root)
  if (state == null) return
  state.connections--
  if (state.connections > 0) return
  roots.delete(root)
  if (state.snapshot == null) return
  const current = takeSnapshot(root)
  if (current == null || current.entries !== state.snapshot.entries || current.mtimeMs === state.snapshot.mtimeMs) return
  try {
    fs.utimesSync(root, fs.statSync(root).atime, state.snapshot.mtimeMs / 1000)
  } catch {
    // Restoring the mtime only helps caches; the store is intact either way.
  }
}

function realpathOrResolve (storeDir: string): string {
  try {
    return fs.realpathSync(storeDir)
  } catch {
    return path.resolve(storeDir)
  }
}

function takeSnapshot (storeDir: string): RootSnapshot | undefined {
  try {
    return {
      mtimeMs: fs.statSync(storeDir).mtimeMs,
      entries: fs.readdirSync(storeDir).sort().join('\0'),
    }
  } catch {
    return undefined
  }
}
