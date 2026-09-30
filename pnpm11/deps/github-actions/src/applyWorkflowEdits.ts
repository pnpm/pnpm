import fs from 'node:fs/promises'

import { PnpmError } from '@pnpm/error'
import writeFileAtomic from 'write-file-atomic'

import { workflowError } from './errors.js'
import type { ActionFile, ActionReference, PlannedUpdate, RepoVersion } from './types.js'

interface WorkflowEdit {
  originalValue: string
  range: readonly [number, number]
  value: string
}

export async function applyWorkflowUpdates (
  updates: PlannedUpdate[],
  selectTarget: (plan: PlannedUpdate) => RepoVersion
): Promise<void> {
  const edits = groupEditsByFile(updates, selectTarget)
  await Promise.all([...edits].map(async ([file, replacements]) => editWorkflowFile(file, replacements)))
}

function groupEditsByFile (
  updates: PlannedUpdate[],
  selectTarget: (plan: PlannedUpdate) => RepoVersion
): Map<ActionFile, WorkflowEdit[]> {
  const edits = new Map<ActionFile, WorkflowEdit[]>()
  for (const plan of updates) {
    const replacements = edits.get(plan.action.file) ?? []
    replacements.push({
      originalValue: plan.action.originalValue,
      range: plan.action.range,
      value: renderTargetValue(plan.action, selectTarget(plan)),
    })
    edits.set(plan.action.file, replacements)
  }
  return edits
}

async function editWorkflowFile (file: ActionFile, replacements: WorkflowEdit[]): Promise<void> {
  let source: string
  try {
    source = await fs.readFile(file.path, 'utf8')
  } catch (err: unknown) {
    throw workflowError('READ', file.path, err)
  }
  if (!replacements.every(({ originalValue, range }) => isUnchanged(source, range, originalValue))) {
    throw new PnpmError('GITHUB_ACTIONS_WORKFLOW_CHANGED', `GitHub Actions workflow ${file.path} changed while resolving updates; retry the command`)
  }
  replacements.sort((left, right) => right.range[0] - left.range[0])
  for (const replacement of replacements) {
    source = source.slice(0, replacement.range[0]) + replacement.value + source.slice(replacement.range[1])
  }
  try {
    await writeFileAtomic(file.path, source)
  } catch (err: unknown) {
    throw workflowError('WRITE', file.path, err)
  }
}

function isUnchanged (source: string, [start, end]: readonly [number, number], originalValue: string): boolean {
  return start >= 0 && start <= end && end <= source.length && source.slice(start, end) === originalValue
}

function renderTargetValue (action: ActionReference, target: RepoVersion): string {
  const oldReference = `${action.name}@${action.ref}`
  const newReference = `${action.name}@${target.commit}`
  const value = action.originalValue.replace(oldReference, newReference)
  if (action.commentVersion != null) {
    const referenceIndex = value.indexOf(newReference)
    const commentSearchStart = referenceIndex === -1 ? 0 : referenceIndex + newReference.length
    return value.slice(0, commentSearchStart) + value.slice(commentSearchStart).replace(action.commentVersion, target.tag)
  }
  const comment = value.indexOf(' #')
  if (comment === -1) {
    return action.flowStyle
      ? `${value.trimEnd()} # ${target.tag}\n${action.indentation}`
      : `${value} # ${target.tag}`
  }
  return `${value.slice(0, comment + 2)}${target.tag} ${value.slice(comment + 2).trimStart()}`
}
