import fs from 'node:fs/promises'
import path from 'node:path'

import { summarizeTarball } from '../tarball/summarizeTarball.js'
import { createStageContext } from './context.js'
import { requireStageId } from './parsing.js'
import { normalizePackageName, renderTarballSummary } from './rendering.js'
import { stageRequest } from './request.js'
import type { StageOptions } from './types.js'

export async function stageDownload (opts: StageOptions, params: string[]): Promise<string> {
  const stageId = requireStageId(params, 'download')
  const context = createStageContext(opts)
  const response = await stageRequest(context, {
    url: new URL(`-/stage/${stageId}/tarball`, context.registry).href,
    init: { method: 'GET' },
    action: `download staged package ${stageId}`,
  })
  const tarballData = Buffer.from(await response.arrayBuffer())
  const summary = await summarizeTarball(tarballData)
  const filename = `${normalizePackageName(summary.name)}-${summary.version}-${stageId}.tgz`
  const downloadedSummary = { ...summary, filename }
  await fs.writeFile(path.resolve(opts.dir ?? process.cwd(), filename), tarballData)

  if (opts.json) return JSON.stringify({ [summary.name]: downloadedSummary }, null, 2)
  return `${renderTarballSummary(downloadedSummary)}\n${filename}`
}
