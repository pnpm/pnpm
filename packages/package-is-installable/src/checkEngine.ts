import semver = require('semver')

class UnsupportedEngineError extends Error {
  public code: 'ERR_PNPM_UNSUPPORTED_ENGINE' = 'ERR_PNPM_UNSUPPORTED_ENGINE'
  public wanted: WantedEngine
  public current: Engine

  constructor (packageId: string, wanted: WantedEngine, current: Engine) {
    super(`Unsupported engine for ${packageId}: wanted: ${JSON.stringify(wanted)} (current: ${JSON.stringify(current)})`)
    this.wanted = wanted
    this.current = current
  }
}

export default function checkEngine (
  packageId: string,
  wantedEngine: WantedEngine,
  currentEngine: Engine,
) {
  if (!wantedEngine) return null
  if (
    (wantedEngine.node && !semver.satisfies(currentEngine.node, wantedEngine.node)) ||
    (wantedEngine.pnpm && !semver.satisfies(currentEngine.pnpm, wantedEngine.pnpm))
  ) {
    return new UnsupportedEngineError(packageId, wantedEngine, currentEngine)
  }
  return null
}

export type Engine = {
  node: string,
  pnpm: string,
}

export type WantedEngine = {
  node?: string,
  pnpm?: string,
}
