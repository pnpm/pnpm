import { PnpmError } from '@pnpm/error'

import { createStageContext } from './context.js'
import { fetchStageItems } from './items.js'
import { parseStagePackageSpec } from './parsing.js'
import { renderStageItem } from './rendering.js'
import type { StageOptions } from './types.js'

export async function stageList (opts: StageOptions, params: string[]): Promise<string> {
  const packageFilter = parsePackageFilter(params[0])

  const context = createStageContext(opts, packageFilter)
  const items = await fetchStageItems(context, packageFilter)

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
