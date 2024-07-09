import { type URL } from 'url'

export function removePort (urlObj: URL): string {
  if (urlObj.port === '') return urlObj.href
  urlObj.port = ''
  return urlObj.toString()
}
