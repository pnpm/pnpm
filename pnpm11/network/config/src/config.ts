import { nerfDart } from '@pnpm/config.registry-auth-key'

function getMaxParts (uris: string[]) {
  return uris.reduce((max, uri) => {
    const parts = uri.split('/').length
    return parts > max ? parts : max
  }, 0)
}

export function pickSettingByUrl<Setting> (
  generic: { [key: string]: Setting } | undefined,
  uri: string,
  opts: { skipFalsyValues?: boolean } = {}
): Setting | undefined {
  if (!generic) return undefined
  const exact = findSetting(generic, uri, opts.skipFalsyValues)
  if (exact != null) return exact.value
  const nerf = nerfDart(uri)
  const withoutPort = removePort(new URL(uri))
  const nerfMatch = findSetting(generic, nerf, opts.skipFalsyValues)
  if (nerfMatch != null) return nerfMatch.value
  const withoutPortMatch = findSetting(generic, withoutPort, opts.skipFalsyValues)
  if (withoutPortMatch != null) return withoutPortMatch.value
  const maxParts = getMaxParts(Object.keys(generic))
  const parts = nerf.split('/')
  for (let partCount = Math.min(parts.length, maxParts) - 1; partCount >= 3; partCount--) {
    const key = `${parts.slice(0, partCount).join('/')}/`
    const parentMatch = findSetting(generic, key, opts.skipFalsyValues)
    if (parentMatch != null) return parentMatch.value
  }
  if (withoutPort !== uri) {
    return pickSettingByUrl(generic, withoutPort, opts)
  }
  return undefined
}

function findSetting<Setting> (
  settings: { [key: string]: Setting },
  key: string,
  skipFalsyValues: boolean | undefined
): { value: Setting } | undefined {
  if (!Object.hasOwn(settings, key)) return undefined
  const value = settings[key]
  return skipFalsyValues && !value ? undefined : { value }
}

export function isNoProxy (uri: string, noProxy: boolean | string | undefined): boolean {
  if (typeof noProxy !== 'string') return noProxy === true
  const hostLabels = new URL(uri).hostname.split('.').filter(Boolean).reverse()
  return noProxy.split(',').some((entry) => {
    const entryLabels = entry.trim().split('.').filter(Boolean).reverse()
    return entryLabels.length > 0 && entryLabels.every((label, index) => hostLabels[index] === label)
  })
}

function removePort (url: URL): string {
  if (url.port === '') return url.href
  url.port = ''
  if (!url.pathname.endsWith('/')) url.pathname += '/'
  return url.href
}
