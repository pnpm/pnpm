import url = require('url')
import mem = require('mem')
import getHost from './getHost'

export default mem(function (registryUrl: string): string {
  const host = getHost(registryUrl)
  return escapeHost(host)
})

export function escapeHost (host: string) {
  return host.replace(':', '+')
}
