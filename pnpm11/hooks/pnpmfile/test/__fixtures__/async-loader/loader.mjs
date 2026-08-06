export const PROBE_SPECIFIER = 'pnpm-async-loader-probe:'

export function resolve (specifier, context, nextResolve) {
  if (specifier === PROBE_SPECIFIER) {
    throw new Error('the asynchronous loader is registered')
  }
  return nextResolve(specifier, context)
}
