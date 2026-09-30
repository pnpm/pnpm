import { nerfDart } from '@pnpm/config.registry-auth-key'

function getMaxParts (uris: string[]) {
  return uris.reduce((max, uri) => {
    const parts = uri.split('/').length
    return parts > max ? parts : max
  }, 0)
}

export function pickSettingByUrl<Setting> (
  generic: { [key: string]: Setting } | undefined,
  uri: string
): Setting | undefined {
  if (!generic) return undefined
  if (Object.hasOwn(generic, uri)) return generic[uri]
  const nerf = nerfDart(uri)
  const withoutPort = removePort(new URL(uri))
  if (Object.hasOwn(generic, nerf)) return generic[nerf]
  if (Object.hasOwn(generic, withoutPort)) return generic[withoutPort]
  const maxParts = getMaxParts(Object.keys(generic))
  const parts = nerf.split('/')
  for (let partCount = Math.min(parts.length, maxParts) - 1; partCount >= 3; partCount--) {
    const key = `${parts.slice(0, partCount).join('/')}/`
    if (Object.hasOwn(generic, key)) {
      return generic[key]
    }
  }
  if (withoutPort !== uri) {
    return pickSettingByUrl(generic, withoutPort)
  }
  return undefined
}

function removePort (url: URL): string {
  if (url.port === '') return url.href
  url.port = ''
  if (!url.pathname.endsWith('/')) url.pathname += '/'
  return url.href
}
