import { PnpmError } from '@pnpm/error'
import { type SupportedArchitectures } from '@pnpm/types'
import { familySync as getLibcFamilySync } from 'detect-libc'

const currentLibc = getLibcFamilySync() ?? 'unknown'

export class UnsupportedPlatformError extends PnpmError {
  public wanted: WantedPlatform
  public current: Platform

  constructor (packageId: string, wanted: WantedPlatform, current: Platform) {
    super('UNSUPPORTED_PLATFORM', `Unsupported platform for ${packageId}: wanted ${JSON.stringify(wanted)} (current: ${JSON.stringify(current)})`)
    this.wanted = wanted
    this.current = current
  }
}

export function checkPlatform (
  packageId: string,
  wantedPlatform: WantedPlatform,
  supportedArchitectures?: SupportedArchitectures
): UnsupportedPlatformError | null {
  const current = {
    os: dedupeCurrent(process.platform, supportedArchitectures?.os ?? ['current']),
    cpu: dedupeCurrent(process.arch, supportedArchitectures?.cpu ?? ['current']),
    libc: dedupeCurrent(currentLibc, supportedArchitectures?.libc ?? ['current']),
  }

  const { platform, arch } = process
  let osOk = true; let cpuOk = true; let libcOk = true

  if (wantedPlatform.os) {
    osOk = checkList(current.os, wantedPlatform.os)
  }
  if (wantedPlatform.cpu) {
    cpuOk = checkList(current.cpu, wantedPlatform.cpu)
  }
  if (wantedPlatform.libc && currentLibc !== 'unknown') {
    libcOk = checkList(current.libc, wantedPlatform.libc)
  }

  if (!osOk || !cpuOk || !libcOk) {
    return new UnsupportedPlatformError(packageId, wantedPlatform, { os: platform, cpu: arch, libc: currentLibc })
  }
  return null
}

export interface Platform {
  cpu: string | string[]
  os: string | string[]
  libc: string | string[]
}

export type WantedPlatform = Partial<Platform>

function checkList (value: string | string[], list: string | string[]): boolean {
  let tmp
  let match = false
  let blc = 0

  if (typeof list === 'string') {
    list = [list]
  }
  if (list.length === 1 && list[0] === 'any') {
    return true
  }
  const values = Array.isArray(value) ? value : [value]
  for (const value of values) {
    for (let i = 0; i < list.length; ++i) {
      tmp = list[i]
      if (tmp[0] === '!') {
        tmp = tmp.slice(1)
        if (tmp === value) {
          return false
        }
        ++blc
      } else {
        match = match || tmp === value
      }
    }
  }
  return match || blc === list.length
}

function dedupeCurrent (current: string, supported: string[]): string[] {
  return supported.map((supported) => supported === 'current' ? current : supported)
}
