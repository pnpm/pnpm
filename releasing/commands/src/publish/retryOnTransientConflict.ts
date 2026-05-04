export interface TransientConflictRetryContext {
  setTimeout: (cb: () => void, ms: number) => void
  globalInfo: (message: string) => void
}

export interface TransientConflictRetryConfig {
  retries?: number
  factor?: number
  minTimeout?: number
  maxTimeout?: number
}

export interface RetryOnTransientConflictParams<T> {
  context: TransientConflictRetryContext
  config?: TransientConflictRetryConfig
  operation: () => Promise<T>
}

const DEFAULT_RETRIES = 2
const DEFAULT_FACTOR = 10
const DEFAULT_MIN_TIMEOUT = 10_000
const DEFAULT_MAX_TIMEOUT = 60_000

/**
 * Retries the operation when the npm registry responds with a transient
 * 409 Conflict ("Failed to save packument"). The npm registry returns this
 * error when a publish lands while a previous write for the same package
 * is still being processed; waiting briefly and retrying typically resolves it.
 */
export async function retryOnTransientConflict<T> ({
  context,
  config,
  operation,
}: RetryOnTransientConflictParams<T>): Promise<T> {
  const retries = config?.retries ?? DEFAULT_RETRIES
  const factor = config?.factor ?? DEFAULT_FACTOR
  const minTimeout = config?.minTimeout ?? DEFAULT_MIN_TIMEOUT
  const maxTimeout = config?.maxTimeout ?? DEFAULT_MAX_TIMEOUT

  for (let attempt = 0; ; attempt++) {
    try {
      return await operation() // eslint-disable-line no-await-in-loop
    } catch (error) {
      if (!isTransientPublishConflict(error) || attempt >= retries) throw error
      const delay = Math.min(maxTimeout, minTimeout * Math.pow(factor, attempt))
      context.globalInfo(
        `The npm registry returned a transient 409 Conflict (the previous publish has not finished processing). Retrying in ${Math.round(delay / 1000)}s (attempt ${attempt + 1} of ${retries})...`
      )
      // eslint-disable-next-line no-await-in-loop
      await new Promise<void>(resolve => {
        context.setTimeout(() => {
          resolve()
        }, delay)
      })
    }
  }
}

function isTransientPublishConflict (error: unknown): boolean {
  if (error == null || typeof error !== 'object') return false
  if ('statusCode' in error && (error as { statusCode?: unknown }).statusCode === 409) return true
  if ('code' in error && (error as { code?: unknown }).code === 'E409') return true
  return false
}
