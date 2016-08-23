'use strict'
const link = require('../api/link')

module.exports = (input, opts) => {
  if (!input || !input.length) {
    return link.linkToGlobal(opts)
  }
  if (input[0].indexOf('.') === 0) {
    return link.linkFromRelative(input[0], opts)
  }
  return link.linkFromGlobal(input[0], opts)
}
