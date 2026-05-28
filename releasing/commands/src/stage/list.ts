import { PnpmError } from '@pnpm/error'

import { createStageContext } from './context.js'
import { parseStagePackageSpec } from './parsing.js'
import { renderStageItem } from './rendering.js'
import { stageJsonRequest } from './request.js'
import type { StageItem, StageListResponse, StageOptions } from './types.js'

const PER_PAGE = 100

export async function stageList (opts: StageOptions, params: string[]): Promise<string> {
  const packageFilter = parsePackageFilter(params[0])

  const context = createStageContext(opts, packageFilter)
  const items: StageItem[] = []
  let page = 0
  while (true) {
    const url = new URL('-/stage', context.registry)
    url.searchParams.set('page', page.toString())
    url.searchParams.set('perPage', PER_PAGE.toString())
    if (packageFilter) {
      url.searchParams.set('package', packageFilter)
    }
    // eslint-disable-next-line no-await-in-loop
    const res = await stageJsonRequest<StageListResponse>(context, { url: url.href, action: 'list staged packages' })
    items.push(...res.items)
    if (items.length >= res.total || res.items.length < PER_PAGE) break
    page++
  }

  if (opts.json) return JSON.stringify(items, null, 2)
  if (items.length === 0) {
    return packageFilter
      ? `No staged versions of package name "${packageFilter}".`
      : 'No staged packages found.'
  }
  return items.map(renderStageItem).join('\n\n')
}

function parsePackageFilter (rawSpec: string | undefined): string | undefined {
  if (!rawSpec) return undefined
  const spec = parseStagePackageSpec(rawSpec)
  if (spec.rawSpec !== '' && spec.rawSpec !== '*') {
    throw new PnpmError('STAGE_VERSION_SPECIFIER_UNSUPPORTED', 'Version specifiers are not supported for listing staged packages')
  }
  return spec.name
}
