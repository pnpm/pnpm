module.exports = function (pkg) {
  return pkg.name.replace('/', '!') + '@' + pkg.version
}
