import url = require('url')
import getHost from './getHost'

export default function (registryUrl: string): string {
  const host = getHost(registryUrl)
  return escapeHost(host)
}

export function escapeHost (host: string) {
  return host.replace(':', '+')
}
