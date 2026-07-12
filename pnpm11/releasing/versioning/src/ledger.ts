import fs from 'node:fs/promises'
import path from 'node:path'
import util from 'node:util'

import { PnpmError } from '@pnpm/error'
import * as yaml from 'yaml'

import { CHANGES_DIR } from './intents.js'

export const LEDGER_FILENAME = 'ledger.yaml'

/**
 * The committed, append-only record of consumed change intents: maps
 * `<package name>@<released version>` to the ids of the intent files consumed
 * by that release. Consumption is scoped per package — an intent file is fully
 * consumed only once every package it names has an entry — which is what makes
 * cherry-picked releases on maintenance branches and merge-backs safe, and
 * what lets one intent be half-consumed by a package on a prerelease line.
 */
export type Ledger = Record<string, string[]>

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
    throw new PnpmError('INVALID_VERSIONING_LEDGER', `Expected ${ledgerPath} to be a mapping of package@version keys to intent id lists`)
  }
  const ledger: Ledger = {}
  for (const [key, ids] of Object.entries(parsed)) {
    if (!Array.isArray(ids) || ids.some((id) => typeof id !== 'string')) {
      throw new PnpmError('INVALID_VERSIONING_LEDGER', `Invalid entry for ${key} in ${ledgerPath}. Expected a list of intent ids`)
    }
    ledger[key] = ids
  }
  return ledger
}

export async function appendToLedger (workspaceDir: string, newEntries: Ledger): Promise<Ledger> {
  const ledger = await readLedger(workspaceDir)
  if (Object.keys(newEntries).length === 0) return ledger
  for (const [key, ids] of Object.entries(newEntries)) {
    ledger[key] = Array.from(new Set([...(ledger[key] ?? []), ...ids])).sort()
  }
  const sorted = Object.fromEntries(Object.entries(ledger).sort(([left], [right]) => left.localeCompare(right)))
  const changesDir = path.join(workspaceDir, CHANGES_DIR)
  await fs.mkdir(changesDir, { recursive: true })
  await fs.writeFile(path.join(changesDir, LEDGER_FILENAME), yaml.stringify(sorted), 'utf8')
  return ledger
}

export interface PackageConsumption {
  /** Intent ids the ledger records against any released version of the package. */
  allIds: Set<string>
  /** Intent ids recorded only against prerelease versions of the package. */
  prereleaseOnlyIds: Set<string>
}

const EMPTY_CONSUMPTION: PackageConsumption = { allIds: new Set(), prereleaseOnlyIds: new Set() }

/**
 * Indexes the ledger by package name in a single pass. Packages without
 * entries map to an empty consumption, so lookups never miss.
 */
export function buildConsumptionIndex (ledger: Ledger): (pkgName: string) => PackageConsumption {
  const stableIdsByPkg = new Map<string, Set<string>>()
  const prereleaseIdsByPkg = new Map<string, Set<string>>()
  for (const [key, ids] of Object.entries(ledger)) {
    const atIndex = key.lastIndexOf('@')
    if (atIndex <= 0) continue
    const pkgName = key.slice(0, atIndex)
    const version = key.slice(atIndex + 1)
    // Build metadata (after "+") may itself contain hyphens and never makes a
    // version a prerelease.
    const isPrerelease = version.split('+')[0].includes('-')
    const byPkg = isPrerelease ? prereleaseIdsByPkg : stableIdsByPkg
    let idSet = byPkg.get(pkgName)
    if (idSet == null) {
      idSet = new Set()
      byPkg.set(pkgName, idSet)
    }
    for (const id of ids) {
      idSet.add(id)
    }
  }

  const consumptionByPkg = new Map<string, PackageConsumption>()
  for (const pkgName of new Set([...stableIdsByPkg.keys(), ...prereleaseIdsByPkg.keys()])) {
    const stableIds = stableIdsByPkg.get(pkgName) ?? new Set()
    const prereleaseIds = prereleaseIdsByPkg.get(pkgName) ?? new Set()
    consumptionByPkg.set(pkgName, {
      allIds: new Set([...stableIds, ...prereleaseIds]),
      prereleaseOnlyIds: new Set([...prereleaseIds].filter((id) => !stableIds.has(id))),
    })
  }
  return (pkgName) => consumptionByPkg.get(pkgName) ?? EMPTY_CONSUMPTION
}
