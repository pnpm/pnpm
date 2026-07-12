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

export async function appendToLedger (workspaceDir: string, newEntries: Ledger): Promise<void> {
  if (Object.keys(newEntries).length === 0) return
  const ledger = await readLedger(workspaceDir)
  for (const [key, ids] of Object.entries(newEntries)) {
    ledger[key] = Array.from(new Set([...(ledger[key] ?? []), ...ids])).sort()
  }
  const sorted = Object.fromEntries(Object.entries(ledger).sort(([left], [right]) => left.localeCompare(right)))
  const changesDir = path.join(workspaceDir, CHANGES_DIR)
  await fs.mkdir(changesDir, { recursive: true })
  await fs.writeFile(path.join(changesDir, LEDGER_FILENAME), yaml.stringify(sorted), 'utf8')
}

export interface PackageConsumption {
  /** Intent ids the ledger records against any released version of the package. */
  allIds: Set<string>
  /** Intent ids recorded only against prerelease versions of the package. */
  prereleaseOnlyIds: Set<string>
}

export function getPackageConsumption (ledger: Ledger, pkgName: string): PackageConsumption {
  const allIds = new Set<string>()
  const stableIds = new Set<string>()
  const prereleaseIds = new Set<string>()
  const prefix = `${pkgName}@`
  for (const [key, ids] of Object.entries(ledger)) {
    if (!key.startsWith(prefix)) continue
    const version = key.slice(prefix.length)
    const isPrerelease = version.includes('-')
    for (const id of ids) {
      allIds.add(id)
      if (isPrerelease) {
        prereleaseIds.add(id)
      } else {
        stableIds.add(id)
      }
    }
  }
  const prereleaseOnlyIds = new Set([...prereleaseIds].filter((id) => !stableIds.has(id)))
  return { allIds, prereleaseOnlyIds }
}
