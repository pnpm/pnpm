import fs from 'node:fs/promises'
import path from 'node:path'
import util from 'node:util'

import { PnpmError } from '@pnpm/error'
import * as yaml from 'yaml'

import { CHANGES_DIR } from './intents.js'

export const LEDGER_FILENAME = 'ledger.yaml'

/**
 * One consumed release: the workspace-relative directory of the project that
 * released (the engine's unit of identity — package names may collide across
 * workspace projects) and the ids of the intent files the release consumed.
 * The bare id-list shape is accepted when read, for hand-written entries;
 * its project is then resolved by the package name in the entry key, which
 * must be unambiguous.
 */
export type LedgerEntry = string[] | { dir: string, intents: string[] }

/**
 * The committed, append-only record of consumed change intents: maps
 * `<package name>@<released version>` to the released project and the ids of
 * the intent files consumed by that release. Consumption is scoped per
 * project — an intent file is fully consumed only once every project it
 * names has an entry — which is what makes cherry-picked releases on
 * maintenance branches and merge-backs safe, and what lets one intent be
 * half-consumed by a package on a release lane.
 */
export type Ledger = Record<string, LedgerEntry>

export function ledgerEntryIds (entry: LedgerEntry): string[] {
  return Array.isArray(entry) ? entry : entry.intents
}

export async function readLedger (workspaceDir: string): Promise<Ledger> {
  const ledgerPath = path.join(workspaceDir, CHANGES_DIR, LEDGER_FILENAME)
  let content: string
  try {
    content = await fs.readFile(ledgerPath, 'utf8')
  } catch (err: unknown) {
    if (util.types.isNativeError(err) && 'code' in err && err.code === 'ENOENT') {
      return {}
    }
    throw err
  }

  const parsed: unknown = yaml.parse(content) ?? {}
  if (typeof parsed !== 'object' || parsed === null || Array.isArray(parsed)) {
    throw new PnpmError('INVALID_VERSIONING_LEDGER', `Expected ${ledgerPath} to be a mapping of package@version keys to consumed-intent entries`)
  }
  // A null prototype so a key like "__proto__" lands as an own property
  // instead of mutating the prototype.
  const ledger: Ledger = Object.create(null) as Ledger
  for (const [key, entry] of Object.entries(parsed)) {
    const normalized = normalizeLedgerEntry(entry)
    if (normalized == null) {
      throw new PnpmError('INVALID_VERSIONING_LEDGER', `Invalid entry for ${key} in ${ledgerPath}. Expected a list of intent ids, or a mapping with "dir" and "intents"`)
    }
    ledger[key] = normalized
  }
  return ledger
}

/**
 * Null where a list belongs reads as an empty list: a bare `pkg@1.0.0:` or
 * `intents:` key parses as YAML null, and a mapping entry may omit
 * `intents` entirely. Committed ledgers contain such entries for releases
 * that consumed no intents. Returns undefined for entries that are invalid
 * in any shape.
 */
function normalizeLedgerEntry (entry: unknown): LedgerEntry | undefined {
  if (entry == null) return []
  if (Array.isArray(entry)) {
    return entry.every((id) => typeof id === 'string') ? (entry as string[]) : undefined
  }
  if (typeof entry !== 'object') return undefined
  const { dir, intents } = entry as { dir?: unknown, intents?: unknown }
  if (typeof dir !== 'string') return undefined
  if (intents == null) return { dir, intents: [] }
  if (!Array.isArray(intents) || !intents.every((id) => typeof id === 'string')) return undefined
  return { dir, intents: intents as string[] }
}

export async function appendToLedger (
  workspaceDir: string,
  newEntries: Record<string, { dir: string, intents: string[] }>
): Promise<Ledger> {
  const ledger = await readLedger(workspaceDir)
  if (Object.keys(newEntries).length === 0) return ledger
  for (const [key, entry] of Object.entries(newEntries)) {
    const existingIds = ledger[key] != null ? ledgerEntryIds(ledger[key]) : []
    ledger[key] = {
      dir: entry.dir,
      intents: Array.from(new Set([...existingIds, ...entry.intents])).sort(),
    }
  }
  const sorted = Object.fromEntries(Object.entries(ledger).sort(([left], [right]) => left.localeCompare(right)))
  const changesDir = path.join(workspaceDir, CHANGES_DIR)
  await fs.mkdir(changesDir, { recursive: true })
  await fs.writeFile(path.join(changesDir, LEDGER_FILENAME), yaml.stringify(sorted), 'utf8')
  return sorted
}

export interface PackageConsumption {
  /** Intent ids the ledger records against any released version of the project. */
  allIds: Set<string>
  /** Intent ids recorded only against prerelease versions of the project. */
  prereleaseOnlyIds: Set<string>
}

/**
 * Indexes the ledger by workspace-relative project directory in a single
 * pass. Bare id-list entries carry no directory, so their project is
 * resolved from the entry key's package name via `resolveNameDirs`; a name
 * matching several projects cannot be attributed and is an error — write
 * such entries in the `dir`/`intents` shape instead. Entries whose name no
 * longer exists in the workspace are inert. Projects without entries map to
 * an empty consumption, so lookups never miss.
 */
export function buildConsumptionIndex (
  ledger: Ledger,
  resolveNameDirs: (pkgName: string) => string[]
): (projectDir: string) => PackageConsumption {
  const stableIdsByDir = new Map<string, Set<string>>()
  const prereleaseIdsByDir = new Map<string, Set<string>>()
  for (const [key, entry] of Object.entries(ledger)) {
    const atIndex = key.lastIndexOf('@')
    if (atIndex <= 0) continue
    const version = key.slice(atIndex + 1)
    let dir: string
    if (Array.isArray(entry)) {
      const pkgName = key.slice(0, atIndex)
      const dirs = resolveNameDirs(pkgName)
      if (dirs.length === 0) continue
      if (dirs.length > 1) {
        throw new PnpmError(
          'INVALID_VERSIONING_LEDGER',
          `The ledger entry ${key} names ${pkgName}, which matches multiple workspace projects (${dirs.join(', ')}). Rewrite the entry with an explicit "dir".`
        )
      }
      dir = dirs[0]
    } else {
      dir = normalizeProjectDir(entry.dir)
    }
    // Build metadata (after "+") may itself contain hyphens and never makes a
    // version a prerelease.
    const isPrerelease = version.split('+')[0].includes('-')
    const byDir = isPrerelease ? prereleaseIdsByDir : stableIdsByDir
    let idSet = byDir.get(dir)
    if (idSet == null) {
      idSet = new Set()
      byDir.set(dir, idSet)
    }
    for (const id of ledgerEntryIds(entry)) {
      idSet.add(id)
    }
  }

  const consumptionByDir = new Map<string, PackageConsumption>()
  for (const dir of new Set([...stableIdsByDir.keys(), ...prereleaseIdsByDir.keys()])) {
    const stableIds = stableIdsByDir.get(dir) ?? new Set()
    const prereleaseIds = prereleaseIdsByDir.get(dir) ?? new Set()
    consumptionByDir.set(dir, {
      allIds: new Set([...stableIds, ...prereleaseIds]),
      prereleaseOnlyIds: new Set([...prereleaseIds].filter((id) => !stableIds.has(id))),
    })
  }
  return (projectDir) => consumptionByDir.get(projectDir) ?? { allIds: new Set(), prereleaseOnlyIds: new Set() }
}

/**
 * The canonical spelling of a workspace-relative project directory: forward
 * slashes, no leading `./`, no trailing slash.
 */
export function normalizeProjectDir (dir: string): string {
  let normalized = dir.replaceAll('\\', '/')
  while (normalized.startsWith('./')) {
    normalized = normalized.slice(2)
  }
  return normalized.replace(/\/+$/, '')
}
