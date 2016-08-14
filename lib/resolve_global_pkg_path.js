'use strict'
const expandTilde = require('./fs/expand_tilde')

module.exports = globalPath => expandTilde(globalPath || '~/.pnpm')
