import url = require('url')

export default function getHost (rawUrl: string) {
  const urlObj = url.parse(rawUrl)
  if (!urlObj || !urlObj.host) {
    throw new Error(`Couldn't get host from ${rawUrl}`)
  }
  return urlObj.host
}
