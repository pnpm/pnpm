import fs from 'node:fs/promises'
import path from 'node:path'
import util from 'node:util'

import { CHANGES_DIR } from './intents.js'

/**
 * Directory (under the workspace's `.changeset/`) holding the composed
 * changelog sections of releases whose intents are not yet published. In
 * `registry` changelog storage no `CHANGELOG.md` is committed, so a release's
 * section is parked here at `pnpm version -r` time and consumed at publish,
 * when it is prepended to the previously published tarball's changelog. Each
 * file is garbage-collected together with the intents it was composed from,
 * under the same registry-confirmed gate.
 */
export const PENDING_CHANGELOGS_DIR = 'changelogs'

/**
 * A published `package@version` names one artifact, so it is a stable key for
 * its parked section. The only character in the key that a filesystem rejects
 * is the `/` of a scoped name, encoded here as `!` (a character neither a
 * package name nor a semver version can contain, so the mapping is reversible).
 */
function pendingChangelogFilename (pkgName: string, version: string): string {
  return `${pkgName}@${version}`.replaceAll('/', '!') + '.md'
}

export function pendingChangelogPath (workspaceDir: string, pkgName: string, version: string): string {
  return path.join(workspaceDir, CHANGES_DIR, PENDING_CHANGELOGS_DIR, pendingChangelogFilename(pkgName, version))
}

export interface PendingChangelog {
  name: string
  version: string
}

/**
 * The `package@version` of every parked section. The release-driven garbage
 * collector consults this rather than the ledger so it also collects the
 * sections of dependency-propagated releases, which carry no consumed intents
 * and therefore have no ledger entry.
 */
export async function listPendingChangelogs (workspaceDir: string): Promise<PendingChangelog[]> {
  const dir = path.join(workspaceDir, CHANGES_DIR, PENDING_CHANGELOGS_DIR)
  let fileNames: string[]
  try {
    fileNames = await fs.readdir(dir)
  } catch (err: unknown) {
    if (util.types.isNativeError(err) && 'code' in err && err.code === 'ENOENT') {
      return []
    }
    throw err
  }
  const pending: PendingChangelog[] = []
  for (const fileName of fileNames) {
    if (!fileName.endsWith('.md')) continue
    const key = fileName.slice(0, -'.md'.length)
    const atIndex = key.lastIndexOf('@')
    if (atIndex <= 0) continue
    pending.push({ name: key.slice(0, atIndex).replaceAll('!', '/'), version: key.slice(atIndex + 1) })
  }
  return pending
}

export async function writePendingChangelog (workspaceDir: string, pkgName: string, version: string, section: string): Promise<void> {
  const filePath = pendingChangelogPath(workspaceDir, pkgName, version)
  await fs.mkdir(path.dirname(filePath), { recursive: true })
  await fs.writeFile(filePath, section, 'utf8')
}

export async function readPendingChangelog (workspaceDir: string, pkgName: string, version: string): Promise<string | null> {
  try {
    return await fs.readFile(pendingChangelogPath(workspaceDir, pkgName, version), 'utf8')
  } catch (err: unknown) {
    if (util.types.isNativeError(err) && 'code' in err && err.code === 'ENOENT') {
      return null
    }
    throw err
  }
}

/** Removes a parked section. A missing file is not an error — it may already have been collected. */
export async function removePendingChangelog (workspaceDir: string, pkgName: string, version: string): Promise<void> {
  try {
    await fs.rm(pendingChangelogPath(workspaceDir, pkgName, version))
  } catch (err: unknown) {
    if (!(util.types.isNativeError(err) && 'code' in err && err.code === 'ENOENT')) {
      throw err
    }
  }
}
