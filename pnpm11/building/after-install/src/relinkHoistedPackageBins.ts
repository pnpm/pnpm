import { relinkModulesDirPackageBins } from '@pnpm/bins.linker'

import type { RebuildState } from './rebuildTypes.js'

export async function relinkHoistedPackageBins (pkgRoots: string[], state: RebuildState): Promise<void> {
  await relinkModulesDirPackageBins(pkgRoots, state, {
    extraNodePaths: state.ctx.extraNodePaths,
    preferSymlinkedExecutables: state.opts.preferSymlinkedExecutables,
    warn: state.warn,
  })
}
