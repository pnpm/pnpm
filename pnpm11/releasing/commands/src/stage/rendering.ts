import type { PublishSummary } from '../tarball/publishSummary.js'
import type { StageItem } from './types.js'

export { normalizePackageName } from '../tarball/safeTarballFilename.js'

export function renderStageItem (item: StageItem): string {
  const { id, packageName, version, tag, createdAt, actor, actorType, shasum, ...rest } = item
  return renderKeyValues({
    id,
    'package name': packageName,
    version,
    tag,
    'date staged': createdAt,
    'staged by': actorType ? `${actor ?? ''} (${actorType})` : actor,
    shasum,
    ...rest,
  })
}

export function renderTarballSummary (summary: PublishSummary): string {
  return `package: ${summary.name}@${summary.version}
Tarball Contents
${summary.files.map(({ path }) => path).join('\n')}
Tarball Details
name: ${summary.name}
version: ${summary.version}
filename: ${summary.filename}
package size: ${summary.size}
unpacked size: ${summary.unpackedSize}
shasum: ${summary.shasum}
integrity: ${summary.integrity}
total files: ${summary.entryCount}`
}

export function renderStagePublishSummary (
  summary: PublishSummary | { name?: string, version?: string },
  opts: { dryRun: boolean }
): string {
  const id = 'id' in summary && summary.id
    ? summary.id
    : summary.name && summary.version
      ? `${summary.name}@${summary.version}`
      : summary.name ?? '<unknown package>'
  if (opts.dryRun) return `+ ${id} (would stage)`
  if ('stageId' in summary && summary.stageId) {
    return `+ ${id} (staged with id ${summary.stageId})`
  }
  return `+ ${id} (staged)`
}

function renderKeyValues (values: Record<string, unknown>): string {
  return Object.entries(values)
    .flatMap(([key, value]) => value == null ? [] : [`${key}: ${renderValue(value)}`])
    .join('\n')
}

function renderValue (value: unknown): string {
  return typeof value === 'object' ? JSON.stringify(value) : String(value)
}
