import { mergeLockfileChanges } from '@pnpm/lockfile.merger'
import type { LockfileFile, LockfileObject } from '@pnpm/lockfile.types'
import yaml from 'js-yaml'

import { convertToLockfileObject } from './lockfileFormatConverters.js'

const MERGE_CONFLICT_PARENT = '|||||||'
const MERGE_CONFLICT_END = '>>>>>>>'
const MERGE_CONFLICT_THEIRS = '======='
const MERGE_CONFLICT_OURS = '<<<<<<<'

export function autofixMergeConflicts (fileContent: string): LockfileObject {
  const { ours, theirs } = parseMergeFile(fileContent)
  return mergeLockfileChanges(
    convertToLockfileObject(yaml.load(ours) as LockfileFile),
    convertToLockfileObject(yaml.load(theirs) as LockfileFile)
  )
}

interface MergeFileInfo {
  ours: string
  theirs: string
}

type MergeFileSection = 'top' | 'ours' | 'theirs' | 'parent'

function parseMergeFile (fileContent: string): MergeFileInfo {
  const lines = fileContent.split(/[\n\r]+/)
  let state: MergeFileSection = 'top'
  const ours = []
  const theirs = []
  for (const line of lines) {
    const markedSection = getSectionStartedByMarker(line)
    if (markedSection != null) {
      state = markedSection
      continue
    }
    if (state === 'top' || state === 'ours') ours.push(line)
    if (state === 'top' || state === 'theirs') theirs.push(line)
  }
  return { ours: ours.join('\n'), theirs: theirs.join('\n') }
}

function getSectionStartedByMarker (line: string): MergeFileSection | undefined {
  if (line.startsWith(MERGE_CONFLICT_PARENT)) return 'parent'
  if (line.startsWith(MERGE_CONFLICT_OURS)) return 'ours'
  if (line === MERGE_CONFLICT_THEIRS) return 'theirs'
  if (line.startsWith(MERGE_CONFLICT_END)) return 'top'
  return undefined
}

export function isDiff (fileContent: string): boolean {
  return fileContent.includes(MERGE_CONFLICT_OURS) &&
    fileContent.includes(MERGE_CONFLICT_THEIRS) &&
    fileContent.includes(MERGE_CONFLICT_END)
}
