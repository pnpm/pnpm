import type { ResolutionContext } from './resolutionTypes.js'

export function startPackageResolution (ctx: ResolutionContext, depth: number): () => void {
  const activeCount = ctx.packageResolutionBarrier.activeByDepth.get(depth) ?? 0
  ctx.packageResolutionBarrier.activeByDepth.set(depth, activeCount + 1)
  let finished = false
  return () => {
    if (finished) return
    finished = true
    const nextActiveCount = (ctx.packageResolutionBarrier.activeByDepth.get(depth) ?? 1) - 1
    if (nextActiveCount > 0) {
      ctx.packageResolutionBarrier.activeByDepth.set(depth, nextActiveCount)
    } else {
      ctx.packageResolutionBarrier.activeByDepth.delete(depth)
    }
    const waiters = ctx.packageResolutionBarrier.waiters.splice(0)
    for (const resolve of waiters) {
      resolve()
    }
  }
}

export async function waitForPackageResolutionTurn (ctx: ResolutionContext, depth: number): Promise<void> {
  if (!hasActivePackageResolutionBeforeDepth(ctx, depth)) return
  await new Promise<void>((resolve) => ctx.packageResolutionBarrier.waiters.push(resolve))
  return waitForPackageResolutionTurn(ctx, depth)
}

function hasActivePackageResolutionBeforeDepth (ctx: ResolutionContext, depth: number): boolean {
  for (const [activeDepth, activeCount] of ctx.packageResolutionBarrier.activeByDepth) {
    if (activeDepth < depth && activeCount > 0) return true
  }
  return false
}
