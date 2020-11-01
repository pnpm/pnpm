import { Lockfile } from '@pnpm/lockfile-types'
import mergeLockfiles from '@pnpm/merge-lockfiles'
import yaml = require('js-yaml')

const MERGE_CONFLICT_PARENT = '|||||||'
const MERGE_CONFLICT_END = '>>>>>>>'
const MERGE_CONFLICT_THEIRS = '======='
const MERGE_CONFLICT_OURS = '<<<<<<<'

export default function (fileContent: string) {
  const { ours, theirs, base } = parseMergeFile(fileContent)
  return mergeLockfiles({
    base: yaml.safeLoad(base) as Lockfile,
    ours: yaml.safeLoad(ours) as Lockfile,
    theirs: yaml.safeLoad(theirs) as Lockfile,
  })
}

function parseMergeFile (fileContent: string) {
  const lines = fileContent.split(/[\n\r]+/g) as string[]
  let state: 'top' | 'ours' | 'theirs' | 'parent' = 'top'
  let ours = ''
  let theirs = ''
  let base = ''
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
    if (state === 'top' || state === 'ours') ours += line
    if (state === 'top' || state === 'theirs') theirs += line
    if (state === 'top' || state === 'parent') base += line
  }
  return { ours, theirs, base }
}

export function isDiff (fileContent: string) {
  return fileContent.includes(MERGE_CONFLICT_OURS) &&
    fileContent.includes(MERGE_CONFLICT_THEIRS) &&
    fileContent.includes(MERGE_CONFLICT_END)
}
