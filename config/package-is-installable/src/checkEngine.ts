import { PnpmError } from '@pnpm/error'
import semver from 'semver'

export class UnsupportedEngineError extends PnpmError {
  public wanted: WantedEngine
  public current: Engine
  public packageId: string

  constructor (packageId: string, wanted: WantedEngine, current: Engine) {
    super('UNSUPPORTED_ENGINE', `Unsupported engine for ${packageId}: wanted: ${JSON.stringify(wanted)} (current: ${JSON.stringify(current)})`)
    this.packageId = packageId
    this.wanted = wanted
    this.current = current
  }
}

export function checkEngine (
  packageId: string,
  wantedEngine: WantedEngine,
  currentEngine: Engine
): UnsupportedEngineError | null {
  if (!wantedEngine) return null
  const unsatisfiedWanted: WantedEngine = {}
  if (wantedEngine.node && !semver.satisfies(currentEngine.node, wantedEngine.node, { includePrerelease: true })) {
    unsatisfiedWanted.node = wantedEngine.node
  }
  if (currentEngine.pnpm && wantedEngine.pnpm && !semver.satisfies(currentEngine.pnpm, wantedEngine.pnpm, { includePrerelease: true })) {
    unsatisfiedWanted.pnpm = wantedEngine.pnpm
  }
  if (Object.keys(unsatisfiedWanted).length > 0) {
    return new UnsupportedEngineError(packageId, unsatisfiedWanted, currentEngine)
  }
  return null
}

export interface Engine {
  node: string
  pnpm?: string
}

export type WantedEngine = Partial<Engine>
