'use strict'
const hierarchyForPackages = require('dependencies-hierarchy').forPackages

hierarchyForPackages(['graceful-fs', {name: 'pify', range: '2'}], __dirname, {depth: 2})
  .then(tree => {
    console.log(JSON.stringify(tree, null, 2))
  })
