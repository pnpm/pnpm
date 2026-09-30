import { errorMessage } from './errorMessage.js'
import {
  quarantine,
  type QueuedLookup,
  type RestorerContext,
} from './remoteSideEffectsRestorerContext.js'
import {
  type DependencySideEffectsCandidate,
  pnprSupportsSharedSideEffects,
  resolveSharedSideEffects,
  type VerifiedArtifact,
} from './sharedSideEffects.js'

/**
 * How long the first queued candidate waits for company before its lookup
 * leaves. Long enough to gather the packages whose fetches land together,
 * short enough that a lone candidate is not what holds up an install.
 */
const LOOKUP_BATCH_WINDOW = 20

/** Well under the protocol's candidate ceiling, so a batch is never refused. */
const MAX_LOOKUP_BATCH = 512

type LookupContext = RestorerContext<string>

/**
 * The artifact pnpr serves for `candidate`, looked up once per input key and
 * batched with the other candidates asked for around the same time.
 */
export async function lookupArtifact (ctx: LookupContext, candidate: DependencySideEffectsCandidate): Promise<VerifiedArtifact | undefined> {
  let lookup = ctx.lookups.get(candidate.key)
  if (lookup == null) {
    lookup = enqueue(ctx, candidate)
    ctx.lookups.set(candidate.key, lookup)
  }
  return lookup
}

async function enqueue (ctx: LookupContext, candidate: DependencySideEffectsCandidate): Promise<VerifiedArtifact | undefined> {
  let resolve!: (artifact: VerifiedArtifact | undefined) => void
  const promise = new Promise<VerifiedArtifact | undefined>((settle) => {
    resolve = settle
  })
  const queue = ctx.lookupQueue
  queue.queued.push({ candidate, resolve })
  if (queue.queued.length >= MAX_LOOKUP_BATCH) {
    flushNow(ctx)
  } else if (queue.flushTimer == null) {
    queue.flushTimer = setTimeout(() => {
      flushNow(ctx)
    }, LOOKUP_BATCH_WINDOW)
    queue.flushTimer.unref?.()
  }
  return promise
}

function flushNow (ctx: LookupContext): void {
  const queue = ctx.lookupQueue
  if (queue.flushTimer != null) {
    clearTimeout(queue.flushTimer)
    queue.flushTimer = undefined
  }
  const batch = queue.queued
  queue.queued = []
  if (batch.length > 0) void lookupBatch(ctx, batch)
}

async function lookupBatch (ctx: LookupContext, batch: QueuedLookup[]): Promise<void> {
  const resolved = await resolveBatch(ctx, batch)
  for (const { candidate, resolve } of batch) {
    resolve(resolved?.get(candidate.key))
  }
}

async function resolveBatch (ctx: LookupContext, batch: QueuedLookup[]): Promise<Map<string, VerifiedArtifact> | undefined> {
  const { registryUrl, authorization } = ctx
  if (registryUrl == null) return undefined
  ctx.lookupQueue.supported ??= checkSharedSideEffectsSupport(ctx, registryUrl)
  if (!await ctx.lookupQueue.supported) return undefined
  try {
    return await resolveSharedSideEffects({
      registryUrl,
      authorization,
      candidates: batch.map(({ candidate }) => candidate),
      supportedTags: ctx.supportedTags,
      policy: {
        ignoreScripts: false,
        eligiblePackages: ctx.eligiblePackages,
        allowedBuilds: new Set(batch.map(({ candidate }) => candidate.subject.package.name)),
      },
      trustedKeys: ctx.trustedKeys,
      quarantinedEnvelopeDigests: ctx.quarantinedEnvelopeDigests,
      onRejectedArtifact: (rejection) => {
        quarantine(ctx, rejection)
      },
    })
  } catch (err: unknown) {
    ctx.opts.warn?.(`Remote side-effects cache lookup failed: ${errorMessage(err)}`)
    return undefined
  }
}

async function checkSharedSideEffectsSupport (ctx: LookupContext, registryUrl: string): Promise<boolean> {
  try {
    return await pnprSupportsSharedSideEffects({ registryUrl, authorization: ctx.authorization })
  } catch (err: unknown) {
    ctx.opts.warn?.(`Remote side-effects cache handshake failed: ${errorMessage(err)}`)
    return false
  }
}
