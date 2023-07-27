import { URL } from 'url'

export function removePort (originalUrl: string) {
  const urlObj = new URL(originalUrl)
  if (urlObj.port === '') return urlObj.href
  urlObj.port = ''
  return urlObj.toString()
}
