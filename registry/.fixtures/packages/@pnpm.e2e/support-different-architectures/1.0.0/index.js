// @ts-check

function getOptionalDependencies () {
  const { optionalDependencies } = require('./package.json')

  /** @type {Record<string, unknown>} */
  const installed = {}

  /** @type {string[]} */
  const notInstalled = []

  for (const packageName in optionalDependencies) {
    try {
      installed[packageName] = require(`${packageName}/package.json`)
    } catch (error) {
      if (error.code === 'MODULE_NOT_FOUND') {
        notInstalled.push(packageName)
      } else {
        throw error
      }
    }
  }

  notInstalled.sort()

  return { installed, notInstalled }
}

module.exports = {
  getOptionalDependencies,
}
