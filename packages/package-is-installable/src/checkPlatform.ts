class UnsupportedPlatformError extends Error {
  public code: 'ERR_PNPM_UNSUPPORTED_PLATFORM' = 'ERR_PNPM_UNSUPPORTED_PLATFORM'
  public wanted: Platform
  public current: Platform

  constructor (packageId: string, wanted: Platform, current: Platform) {
    super(`Unsupported platform for ${packageId}: wanted ${JSON.stringify(wanted)} (current: ${JSON.stringify(current)})`)
    this.wanted = wanted
    this.current = current
  }
}

export default function checkPlatform (packageId: string, wantedPlatform: Platform) {
  const platform = process.platform
  const arch = process.arch
  let osOk = true
  let cpuOk = true

  if (wantedPlatform.os) {
    osOk = checkList(platform, wantedPlatform.os)
  }
  if (wantedPlatform.cpu) {
    cpuOk = checkList(arch, wantedPlatform.cpu)
  }
  if (!osOk || !cpuOk) {
    return new UnsupportedPlatformError(packageId, wantedPlatform, { os: platform, cpu: arch })
  }
  return null
}

export type Platform = {
  cpu: string | string[],
  os: string | string[],
}

function checkList (value: string, list: string | string[]) {
  let tmp
  let match = false
  let blc = 0
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
