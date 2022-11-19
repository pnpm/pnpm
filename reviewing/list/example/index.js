'use strict'
const pnpmList = require('../lib').default

pnpmList(__dirname, {depth: 2})
  .then(output => {
    console.log(output)
  })
