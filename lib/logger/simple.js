module.exports = function logger () {
  return function (pkg, keypath) {
    var name = pkg.name
      ? (pkg.name + ' ' + pkg.rawSpec)
      : pkg.rawSpec

    var prefix = Array(1 + keypath.length).join('  ')

    console.log('' + prefix + name)
    return function (status, args) {
      console.log('' + prefix + name + ': ' + status)
    }
  }
}
