import { PnpmError } from '@pnpm/error'
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
  wantedPlatform: WantedPlatform
) {
  const { platform, arch } = process
  let osOk = true; let cpuOk = true; let libcOk = true

  if (wantedPlatform.os) {
    osOk = checkList(platform, wantedPlatform.os)
  }
  if (wantedPlatform.cpu) {
    cpuOk = checkList(arch, wantedPlatform.cpu)
  }
  if (wantedPlatform.libc && currentLibc !== 'unknown') {
    libcOk = checkList(currentLibc, wantedPlatform.libc)
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

function checkList (value: string, list: string | string[]) {
  let tmp; let match = false; let blc = 0
  if (typeof list === 'string') {
    list = [list]
  }
  if (list.length === 1 && list[0] === 'any') {
    return true
  }
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
  return match || blc === list.length
}
