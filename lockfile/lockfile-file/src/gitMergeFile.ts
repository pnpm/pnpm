import { type Lockfile } from '@pnpm/lockfile-types'
import { mergeLockfileChanges } from '@pnpm/merge-lockfile-changes'
import yaml from 'js-yaml'
import { revertFromInlineSpecifiersFormatIfNecessary } from './experiments/inlineSpecifiersLockfileConverters'

const MERGE_CONFLICT_PARENT = '|||||||'
const MERGE_CONFLICT_END = '>>>>>>>'
const MERGE_CONFLICT_THEIRS = '======='
const MERGE_CONFLICT_OURS = '<<<<<<<'

export function autofixMergeConflicts (fileContent: string) {
  const { ours, theirs } = parseMergeFile(fileContent)
  return mergeLockfileChanges(
    revertFromInlineSpecifiersFormatIfNecessary(yaml.load(ours) as Lockfile),
    revertFromInlineSpecifiersFormatIfNecessary(yaml.load(theirs) as Lockfile)
  )
}

function parseMergeFile (fileContent: string) {
  const lines = fileContent.split(/[\n\r]+/g)
  let state: 'top' | 'ours' | 'theirs' | 'parent' = 'top'
  const ours = []
  const theirs = []
  while (lines.length > 0) {
    const line = lines.shift() as string
    if (line.startsWith(MERGE_CONFLICT_PARENT)) {
      state = 'parent'
      continue
    }
    if (line.startsWith(MERGE_CONFLICT_OURS)) {
      state = 'ours'
      continue
    }
    if (line === MERGE_CONFLICT_THEIRS) {
      state = 'theirs'
      continue
    }
    if (line.startsWith(MERGE_CONFLICT_END)) {
      state = 'top'
      continue
    }
    if (state === 'top' || state === 'ours') ours.push(line)
    if (state === 'top' || state === 'theirs') theirs.push(line)
  }
  return { ours: ours.join('\n'), theirs: theirs.join('\n') }
}

export function isDiff (fileContent: string) {
  return fileContent.includes(MERGE_CONFLICT_OURS) &&
    fileContent.includes(MERGE_CONFLICT_THEIRS) &&
    fileContent.includes(MERGE_CONFLICT_END)
}
