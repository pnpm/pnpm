import type { StageContext } from './context.js'
import { stageJsonRequest } from './request.js'
import type { StageItem, StageListResponse } from './types.js'

const PER_PAGE = 100
// Fail-safe bound on the pagination loop, so a registry that keeps answering
// full pages with an inflated `total` cannot drive it forever.
const MAX_PAGES = 1000

/**
 * Every staged version the registry reports, optionally narrowed to one
 * package name.
 */
export async function fetchStageItems (context: StageContext, packageFilter?: string): Promise<StageItem[]> {
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
    if (page >= MAX_PAGES) break
  }
  return items
}
