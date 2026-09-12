/// <reference path="../../../__typings__/local.d.ts" />
import fs from 'node:fs/promises'
import path from 'node:path'
import { fileURLToPath } from 'node:url'

import { PnpmError } from '@pnpm/error'
import { readPendingChangelog } from '@pnpm/releasing.versioning'
import { toString as mdastToString } from 'mdast-util-to-string'
import remarkParse from 'remark-parse'
import remarkStringify from 'remark-stringify'
import { unified } from 'unified'

export const BumpLevels = {
  dep: 0,
  patch: 1,
  minor: 2,
  major: 3,
} as const

// The sponsors table is shared with the v12 release job, which cats this same
// fragment onto its RELEASE.md. It is regenerated from pnpm.io's sponsors.json.
// A missing fragment means someone moved it, not that there are no sponsors —
// warn rather than throw, since the workflow falls back to a diagnostic
// description when this script fails, and no sponsors beats no release notes.
const SPONSORS_FRAGMENT = '.github/release-sponsors.md'

async function readSponsors (workspaceDir: string): Promise<string> {
  try {
    return await fs.readFile(path.join(workspaceDir, SPONSORS_FRAGMENT), 'utf8')
  } catch (err: unknown) {
    if ((err as NodeJS.ErrnoException).code !== 'ENOENT') throw err
    console.warn(`::warning::No sponsors fragment at ${SPONSORS_FRAGMENT}; writing the release description without the sponsors table`)
    return ''
  }
}

export async function writeReleaseText (workspaceDir: string): Promise<void> {
  const pnpmDir = path.join(workspaceDir, 'pnpm11/pnpm')
  const pnpm = JSON.parse(await fs.readFile(path.join(pnpmDir, 'package.json'), 'utf8'))
  const changelog = await readPendingChangelog(workspaceDir, pnpm.name, pnpm.version)
  if (changelog == null) {
    throw new PnpmError('MISSING_CHANGELOG', `No pending changelog found for pnpm ${pnpm.version}`)
  }
  const release = getChangelogEntry(changelog, pnpm.version)
  const sponsors = await readSponsors(workspaceDir)
  const content = sponsors === '' ? release.content : `${release.content}\n${sponsors}`
  const releasePath = path.join(workspaceDir, 'RELEASE.md')
  const temporaryPath = `${releasePath}.${process.pid}.tmp`
  await fs.writeFile(temporaryPath, content)
  await fs.rename(temporaryPath, releasePath)
}

interface ChangelogEntry {
  content: string
  highestLevel: number
}

export function getChangelogEntry (changelog: string, version: string): ChangelogEntry {
  const ast = unified().use(remarkParse).parse(changelog)

  let highestLevel: number = BumpLevels.dep

  const nodes = ast['children'] as any[] // eslint-disable-line @typescript-eslint/no-explicit-any
  let headingStartInfo:
  | {
    index: number
    depth: number
  }
  | undefined
  let endIndex: number | undefined

  for (let i = 0; i < nodes.length; i++) {
    const node = nodes[i]
    if (node.type === 'heading') {
      const stringified: string = mdastToString(node)
      const match = stringified.toLowerCase().match(/(major|minor|patch)/)
      if (match !== null) {
        const level = BumpLevels[match[0] as 'major' | 'minor' | 'patch']
        highestLevel = Math.max(level, highestLevel)
      }
      if (headingStartInfo === undefined && stringified === version) {
        headingStartInfo = {
          index: i,
          depth: node.depth,
        }
        continue
      }
      if (
        endIndex === undefined &&
        headingStartInfo !== undefined &&
        headingStartInfo.depth === node.depth
      ) {
        endIndex = i
        break
      }
    }
  }
  if (headingStartInfo == null) {
    throw new PnpmError('MISSING_CHANGELOG_ENTRY', `No changelog entry found for pnpm ${version}`)
  }
  ast['children'] = (ast['children'] as any).slice( // eslint-disable-line @typescript-eslint/no-explicit-any
    headingStartInfo.index + 1,
    endIndex
  )
  return {
    content: unified().use(remarkStringify).stringify(ast),
    highestLevel,
  }
}

// The entry point stays at the bottom: a top-level `await` above a module-level
// constant runs before that constant is initialized, and the release job only
// finds out when the description fails to generate.
if (process.argv[1] != null && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  const dirname = path.dirname(fileURLToPath(import.meta.url))
  await writeReleaseText(process.argv[2] ?? path.resolve(dirname, '../../../..'))
}
