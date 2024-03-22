import semver from 'semver'

import { PnpmError } from '@pnpm/error'
import type { WantedEngine, Engine } from '@pnpm/types'

export class UnsupportedEngineError extends PnpmError {
  public wanted: WantedEngine
  public current: Engine | undefined
  public packageId: string

  constructor(packageId: string, wanted: WantedEngine, current: Engine | undefined) {
    super(
      'UNSUPPORTED_ENGINE',
      `Unsupported engine for ${packageId}: wanted: ${JSON.stringify(wanted)} (current: ${JSON.stringify(current)})`
    )
    this.packageId = packageId
    this.wanted = wanted
    this.current = current
  }
}

export function checkEngine(
  packageId: string,
  wantedEngine: { node?: string | undefined; npm?: string | undefined; pnpm?: string | undefined; },
  currentEngine: Engine | undefined
): UnsupportedEngineError | null {
  if (!wantedEngine) {
    return null
  }

  const unsatisfiedWanted: WantedEngine = {}

  if (
    wantedEngine.node &&
    !semver.satisfies(currentEngine?.node ?? '', wantedEngine.node, {
      includePrerelease: true,
    })
  ) {
    unsatisfiedWanted.node = wantedEngine.node
  }

  if (
    currentEngine?.pnpm &&
    wantedEngine.pnpm &&
    !semver.satisfies(currentEngine.pnpm, wantedEngine.pnpm, {
      includePrerelease: true,
    })
  ) {
    unsatisfiedWanted.pnpm = wantedEngine.pnpm
  }

  if (Object.keys(unsatisfiedWanted).length > 0) {
    return new UnsupportedEngineError(
      packageId,
      unsatisfiedWanted,
      currentEngine
    )
  }

  return null
}
