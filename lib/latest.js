var join = require('path').join
var url = require('url')
var enc = global.encodeURIComponent
var got = require('./got')
var config = require('./config')

module.exports = function (pkg) {
  // { raw: 'rimraf@2', scope: null, name: 'rimraf', rawSpec: '2' || '' }
  return Promise.resolve()
    .then(_ => toUri(pkg))
    .then(ver => {
      console.log(ver);
    })
    // .then(url => got(url))
    // .then(res => {
    //   var body = JSON.parse(res.body)
    //   return body['dist-tags'].latest;
    // })
    .catch(errify)

  function errify (err) {
    if (err.statusCode === 404) {
      throw new Error("Module '" + pkg.raw + "' not found")
    }
    throw err
  }
}

/**
 * Converts package data (from `npa()`) to a URI
 *
 *     toUri({ name: 'rimraf', rawSpec: '2' })
 *     // => 'https://registry.npmjs.org/rimraf/2'
 */

function toUri (pkg) {
  var name

  if (pkg.name.substr(0, 1) === '@') {
    name = '@' + enc(pkg.name.substr(1))
  } else {
    name = enc(pkg.name)
  }

  return url.resolve(config.registry, name)
}
