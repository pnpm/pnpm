import fs from 'node:fs/promises'
import path from 'node:path'
import util from 'node:util'

import { PnpmError } from '@pnpm/error'
import { humanId } from 'human-id'
import * as yaml from 'yaml'

export const CHANGES_DIR = '.changeset'

export const BUMP_TYPES = ['none', 'patch', 'minor', 'major'] as const

export type IntentBumpType = typeof BUMP_TYPES[number]

export type ReleaseBumpType = Exclude<IntentBumpType, 'none'>

export interface ChangeIntent {
  id: string
  filePath: string
  releases: Record<string, IntentBumpType>
  summary: string
}

export function parseChangeIntent (content: string, id: string, filePath: string): ChangeIntent {
  const lines = content.replace(/^\uFEFF/, '').split(/\r?\n/)
  const closingIndex = lines[0]?.trim() === '---'
    ? lines.findIndex((line, index) => index > 0 && line.trim() === '---')
    : -1
  if (closingIndex === -1) {
    throw new PnpmError('INVALID_CHANGE_INTENT', `Change intent file ${filePath} has no YAML frontmatter`)
  }

  let frontmatter: unknown
  try {
    frontmatter = yaml.parse(lines.slice(1, closingIndex).join('\n')) ?? {}
  } catch (err: unknown) {
    throw new PnpmError('INVALID_CHANGE_INTENT', `Change intent file ${filePath} has invalid YAML frontmatter: ${util.types.isNativeError(err) ? err.message : String(err)}`)
  }

  if (typeof frontmatter !== 'object' || frontmatter === null || Array.isArray(frontmatter)) {
    throw new PnpmError('INVALID_CHANGE_INTENT', `Change intent file ${filePath} frontmatter must be a mapping of package names to bump types`)
  }

  const releases: Record<string, IntentBumpType> = {}
  for (const [pkgName, bumpType] of Object.entries(frontmatter)) {
    if (typeof bumpType !== 'string' || !(BUMP_TYPES as readonly string[]).includes(bumpType)) {
      throw new PnpmError('INVALID_CHANGE_INTENT', `Change intent file ${filePath} declares an invalid bump type for ${pkgName}: ${String(bumpType)}. Expected one of ${BUMP_TYPES.join(', ')}`)
    }
    releases[pkgName] = bumpType as IntentBumpType
  }

  return {
    id,
    filePath,
    releases,
    summary: lines.slice(closingIndex + 1).join('\n').trim(),
  }
}

export async function readChangeIntents (workspaceDir: string): Promise<ChangeIntent[]> {
  const changesDir = path.join(workspaceDir, CHANGES_DIR)
  let fileNames: string[]
  try {
    fileNames = await fs.readdir(changesDir)
  } catch (err: unknown) {
    if (util.types.isNativeError(err) && 'code' in err && err.code === 'ENOENT') {
      return []
    }
    throw err
  }

  const intentFileNames = fileNames
    .filter((fileName) => fileName.endsWith('.md') && fileName.toLowerCase() !== 'readme.md')
    .sort()

  return Promise.all(intentFileNames.map(async (fileName) => {
    const filePath = path.join(changesDir, fileName)
    const content = await fs.readFile(filePath, 'utf8')
    return parseChangeIntent(content, fileName.slice(0, -'.md'.length), filePath)
  }))
}

export interface WriteChangeIntentOptions {
  releases: Record<string, IntentBumpType>
  summary: string
}

export async function writeChangeIntent (workspaceDir: string, opts: WriteChangeIntentOptions): Promise<string> {
  const changesDir = path.join(workspaceDir, CHANGES_DIR)
  await fs.mkdir(changesDir, { recursive: true })

  const existing = new Set(await fs.readdir(changesDir))
  let id = humanId({ separator: '-', capitalize: false })
  while (existing.has(`${id}.md`)) {
    id = humanId({ separator: '-', capitalize: false })
  }

  const frontmatterLines = Object.entries(opts.releases)
    .map(([pkgName, bumpType]) => `${JSON.stringify(pkgName)}: ${bumpType}`)
  const content = `---\n${frontmatterLines.join('\n')}\n---\n\n${opts.summary.trim()}\n`
  await fs.writeFile(path.join(changesDir, `${id}.md`), content, 'utf8')
  return id
}
