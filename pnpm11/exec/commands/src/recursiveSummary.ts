import path from 'node:path'

import type { RecursiveSummary } from '@pnpm/cli.utils'
import { writeJsonFile } from 'write-json-file'

export async function writeRecursiveSummary (opts: { dir: string, summary: RecursiveSummary }): Promise<void> {
  await writeJsonFile(path.join(opts.dir, 'pnpm-exec-summary.json'), {
    executionStatus: opts.summary,
  })
}

export function getExecutionDuration (start: [number, number]): number {
  const end = process.hrtime(start)
  return (end[0] * 1e9 + end[1]) / 1e6
}
