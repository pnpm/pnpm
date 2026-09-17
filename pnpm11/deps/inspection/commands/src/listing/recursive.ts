import assert from 'node:assert'
import util from 'node:util'

import type { Config } from '@pnpm/config.reader'
import { renderJson } from '@pnpm/deps.inspection.list'
import { logger } from '@pnpm/logger'
import type { IncludedDependencies, Project } from '@pnpm/types'

import { determineReportAs } from './common.js'
import { loadProjects, render } from './list.js'

export async function listRecursive (
  pkgs: Project[],
  params: string[],
  opts: Pick<Config, 'lockfileDir' | 'virtualStoreDirMaxLength'> & {
    depth?: number
    include: IncludedDependencies
    long?: boolean
    json?: boolean
    parseable?: boolean
    lockfileDir?: string
    checkWantedLockfileOnly?: boolean
    onlyProjects?: boolean
  }
): Promise<string> {
  const depth = opts.depth ?? 0
  if (opts.lockfileDir) {
    return render(pkgs.map((pkg) => pkg.rootDir), params, {
      ...opts,
      alwaysPrintRootPackage: depth === -1,
      lockfileDir: opts.lockfileDir,
    })
  }
  if (determineReportAs(opts) === 'json') {
    const projects = await Promise.all(pkgs.map(({ rootDir }) =>
      withProjectError(rootDir, () => loadProjects([rootDir], params, {
        ...opts,
        lockfileDir: rootDir,
      }))
    ))
    return renderJson(projects.flat(), { depth, long: opts.long ?? false, search: params.length > 0 })
  }
  const outputs = (await Promise.all(pkgs.map(({ rootDir }) =>
    withProjectError(rootDir, () => render([rootDir], params, {
      ...opts,
      alwaysPrintRootPackage: depth === -1,
      lockfileDir: rootDir,
    }))
  ))).filter(Boolean)
  if (outputs.length === 0) return ''

  const joiner = typeof depth === 'number' && depth > -1 ? '\n\n' : '\n'
  return outputs.join(joiner)
}

async function withProjectError<Result> (rootDir: string, action: () => Promise<Result>): Promise<Result> {
  try {
    return await action()
  } catch (err: unknown) {
    assert(util.types.isNativeError(err))
    const errWithPrefix = Object.assign(err, { prefix: rootDir })
    logger.info(errWithPrefix)
    throw errWithPrefix
  }
}
