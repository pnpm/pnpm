import fs from 'node:fs/promises'
import path from 'node:path'
import util from 'node:util'

import type { PlannedRelease } from './assembleReleasePlan.js'
import type { ReleaseBumpType } from './intents.js'

const SECTION_TITLES: Record<ReleaseBumpType, string> = {
  major: 'Major Changes',
  minor: 'Minor Changes',
  patch: 'Patch Changes',
}

export function composeChangelogSection (release: PlannedRelease): string {
  const entriesByBump: Record<ReleaseBumpType, string[]> = { major: [], minor: [], patch: [] }
  for (const intent of release.intents) {
    const bumpType = intent.releases[release.name]
    if (bumpType === 'none' || intent.summary === '') continue
    entriesByBump[bumpType].push(formatListItem(intent.summary))
  }
  if (release.dependencyUpdates.length > 0) {
    const depLines = release.dependencyUpdates.map((dep) => `  - ${dep.name}@${dep.newVersion}`)
    entriesByBump.patch.push(`- Updated dependencies:\n${depLines.join('\n')}`)
  }

  const parts: string[] = [`## ${release.newVersion}`]
  for (const bumpType of ['major', 'minor', 'patch'] as const) {
    if (entriesByBump[bumpType].length === 0) continue
    parts.push(`### ${SECTION_TITLES[bumpType]}`)
    parts.push(entriesByBump[bumpType].join('\n\n'))
  }
  return `${parts.join('\n\n')}\n`
}

function formatListItem (summary: string): string {
  const [firstLine, ...restLines] = summary.split('\n')
  const rest = restLines.map((line) => (line === '' ? '' : `  ${line}`))
  return ['- ' + firstLine, ...rest].join('\n')
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

  let content: string
  if (existing == null) {
    content = `# ${pkgName}\n\n${section}`
  } else {
    const newlineIndex = existing.indexOf('\n')
    const firstLine = newlineIndex === -1 ? existing : existing.slice(0, newlineIndex)
    if (firstLine.startsWith('# ')) {
      const body = (newlineIndex === -1 ? '' : existing.slice(newlineIndex + 1)).replace(/^[\r\n]+/, '')
      content = `${firstLine}\n\n${section}${body === '' ? '' : `\n${body}`}`
    } else {
      content = `${section}\n${existing}`
    }
  }
  await fs.writeFile(changelogPath, content, 'utf8')
}
