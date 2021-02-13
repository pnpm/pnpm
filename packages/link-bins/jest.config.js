import config from '../../jest.config.js'

export default {
  ...config,
  // Shallow so fixtures aren't matched
  testMatch: ["**/test/*.[jt]s?(x)"],
}
