import { removePort } from '../src/helpers/removePort'

describe('removePort()', () => {
  it('does not mutate the url if no port is found', () => {
    const urlString = 'https://custom.domain.com/npm/-/foo-1.0.0.tgz'
    expect(removePort(urlString)).toEqual(urlString)

    const urlStringWithTrailingSlash = 'https://custom.domain.com/npm/'
    expect(removePort(urlStringWithTrailingSlash)).toEqual(
      urlStringWithTrailingSlash
    )
  })

  it('removes ports from urls with https | https | ws | wss protocols', () => {
    const portsToTest = [1, 8888, 8080, 8081, 65535]
    const protocols = ['http', 'https', 'ws', 'wss']

    const getUrl = (port: number, protocol: string) =>
      `${protocol}://custom.domain.com:${port}/artifactory/api/npm/npm-virtual/-/foo-1.0.0.tgz`

    const expectedOutput = (protocol: string) =>
      `${protocol}://custom.domain.com/artifactory/api/npm/npm-virtual/-/foo-1.0.0.tgz`

    portsToTest.forEach((port: number) => {
      protocols.forEach((protocol) => {
        expect(removePort(getUrl(port, protocol))).toEqual(
          expectedOutput(protocol)
        )
      })
    })
  })

  it('removes ports from valid urls with http, https, ws, wss protocols', () => {
    const portsWithEmptyReturns = new Map([
      ['http', 80],
      ['https', 443],
      ['ws', 80],
      ['wss', 443],
    ])

    const getUrl = (port: number, protocol: string) =>
      `${protocol}://custom.domain.com:${port}/artifactory/api/npm/npm-virtual/-/foo-1.0.0.tgz`

    const expectedOutput = (protocol: string) =>
      `${protocol}://custom.domain.com/artifactory/api/npm/npm-virtual/-/foo-1.0.0.tgz`

    portsWithEmptyReturns.forEach((value: number, protocol) => {
      expect(removePort(getUrl(value, protocol))).toEqual(
        expectedOutput(protocol)
      )
    })
  })

  /**
   * @description intentially mismatch the port
   * https|wss set to 443
   * http|ws set to 80
   *
   * @tests regexp loopholes of (80:443)
   */
  it('removes the ports from urls with protocol port mismatches', () => {
    const mistmatchProtocolPorts = new Map([
      ['http', 443],
      ['ws', 443],
      ['https', 80],
      ['wss', 80],
    ])

    const getUrl = (port: number, protocol: string) =>
      `${protocol}://custom.domain.com:${port}/artifactory/api/npm/npm-virtual/-/foo-1.0.0.tgz`
    const expectedOutput = (protocol: string) =>
      `${protocol}://custom.domain.com/artifactory/api/npm/npm-virtual/-/foo-1.0.0.tgz`
    mistmatchProtocolPorts.forEach((value: number, protocol) => {
      expect(removePort(getUrl(value, protocol))).toEqual(
        expectedOutput(protocol)
      )
    })
  })
})
