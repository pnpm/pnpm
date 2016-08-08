module.exports = function getSaveType (opts) {
  if (opts.save) return 'dependencies'
  if (opts.saveDev) return 'devDependencies'
  if (opts.saveOptional) return 'optionalDependencies'
}
