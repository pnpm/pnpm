import fs from 'node:fs/promises'
import path from 'node:path'
import util from 'node:util'

import { isDirRef, type PlannedRelease } from './assembleReleasePlan.js'
import type { IntentBumpType, ReleaseBumpType } from './intents.js'
import { normalizeProjectDir } from './ledger.js'

const SECTION_TITLES: Record<ReleaseBumpType, string> = {
  major: 'Major Changes',
  minor: 'Minor Changes',
  patch: 'Patch Changes',
}

export function composeChangelogSection (release: PlannedRelease): string {
  const entriesByBump: Record<ReleaseBumpType, string[]> = { major: [], minor: [], patch: [] }
  for (const intent of release.intents) {
    const bumpType = releaseBumpFor(intent.releases, release)
    if (bumpType == null || bumpType === 'none' || intent.summary === '') continue
    entriesByBump[bumpType].push(formatListItem(intent.summary))
  }
  if (release.dependencyUpdates.length > 0) {
    const depLines = release.dependencyUpdates.map((dep) => `  - ${dep.name}@${dep.newVersion}`)
    entriesByBump.patch.push(`- Updated dependencies\n${depLines.join('\n')}`)
  }

  const parts: string[] = [`## ${release.newVersion}`]
  for (const bumpType of ['major', 'minor', 'patch'] as const) {
    if (entriesByBump[bumpType].length === 0) continue
    parts.push(`### ${SECTION_TITLES[bumpType]}`)
    parts.push(entriesByBump[bumpType].join('\n'))
  }
  return `${parts.join('\n\n')}\n`
}

/**
 * The bump an intent declares for this release, whichever way the intent
 * references the project — by name (sound only when unambiguous, which plan
 * assembly guarantees) or by directory.
 */
function releaseBumpFor (releases: Record<string, IntentBumpType>, release: PlannedRelease): IntentBumpType | undefined {
  for (const [ref, bumpType] of Object.entries(releases)) {
    if (ref === release.name) return bumpType
    if (isDirRef(ref) && normalizeProjectDir(ref) === release.dir) return bumpType
  }
  return undefined
}

function formatListItem (summary: string): string {
  const [firstLine, ...restLines] = summary.split('\n')
  const rest = restLines.map((line) => (line === '' ? '' : `  ${line}`))
  return ['- ' + firstLine, ...rest].join('\n')
}

/**
 * Places `section` at the top of a package's changelog: under the existing
 * `# <name>` title when `existing` has one, or under a freshly created title
 * when `existing` is `null`. This is the composition used both to write a
 * committed CHANGELOG.md (`repository` storage) and to build the changelog
 * packed into a published tarball on top of the previous version's
 * (`registry` storage).
 */
export function renderChangelog (existing: string | null, pkgName: string, section: string): string {
  if (existing == null) {
    return `# ${pkgName}\n\n${section}`
  }
  const newlineIndex = existing.indexOf('\n')
  const firstLine = newlineIndex === -1 ? existing : existing.slice(0, newlineIndex)
  if (firstLine.startsWith('# ')) {
    const body = (newlineIndex === -1 ? '' : existing.slice(newlineIndex + 1)).replace(/^[\r\n]+/, '')
    return `${firstLine}\n\n${section}${body === '' ? '' : `\n${body}`}`
  }
  return `${section}\n${existing}`
}

export async function prependChangelogSection (pkgDir: string, pkgName: string, section: string): Promise<void> {
  const changelogPath = path.join(pkgDir, 'CHANGELOG.md')
  let existing: string | null = null
  try {
    existing = await fs.readFile(changelogPath, 'utf8')
  } catch (err: unknown) {
    if (!(util.types.isNativeError(err) && 'code' in err && err.code === 'ENOENT')) {
      throw err
    }
  }
  await fs.writeFile(changelogPath, renderChangelog(existing, pkgName, section), 'utf8')
}
