import * as publishCommand from '../publish/publish.js'
import type { PublishSummary } from '../tarball/publishSummary.js'
import { renderStagePublishSummary } from './rendering.js'
import type { StageOptions } from './types.js'

type StagePublishResult = { exitCode?: number, output?: string } | undefined

export async function stagePublish (opts: StageOptions, params: string[]): Promise<StagePublishResult> {
  const result = await publishCommand.publish({
    ...opts,
    stage: true,
  }, params)

  if (opts.json) {
    if (result.publishSummary) {
      return { output: JSON.stringify(keyByPackageName([result.publishSummary]), null, 2), exitCode: 0 }
    }
    if (result.publishedPackages) {
      return { output: JSON.stringify(keyByPackageName(result.publishedPackages), null, 2), exitCode: result.exitCode ?? 0 }
    }
  }

  const publishedPackages = result.publishSummary
    ? [result.publishSummary]
    : result.publishedPackages ?? []
  if (publishedPackages.length > 0) {
    return {
      output: publishedPackages.map((summary) => renderStagePublishSummary(summary, { dryRun: opts.dryRun === true })).join('\n'),
      exitCode: result.exitCode ?? 0,
    }
  }
  if (result.exitCode) return { exitCode: result.exitCode }
  return undefined
}

type PublishedPackage = PublishSummary | { name?: string, version?: string }

function keyByPackageName (packages: PublishedPackage[]): Record<string, PublishedPackage> {
  const keyed: Record<string, PublishedPackage> = {}
  for (const pkg of packages) {
    const key = pkg.name ?? ('id' in pkg ? pkg.id : undefined)
    if (key) keyed[key] = pkg
  }
  return keyed
}
