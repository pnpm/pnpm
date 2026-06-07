#! /usr/bin/env node
const { getOptionalDependencies } = require('../index.js')
const object = getOptionalDependencies()
const json = JSON.stringify(object, undefined, 2)
console.log(json)
