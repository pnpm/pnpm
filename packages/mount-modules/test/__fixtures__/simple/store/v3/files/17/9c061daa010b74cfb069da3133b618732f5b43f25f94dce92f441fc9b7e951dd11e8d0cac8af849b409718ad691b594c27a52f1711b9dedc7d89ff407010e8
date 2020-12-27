var ini = require('ini'),
    path = require('path'),
    fs = require('fs');

module.exports = function (gitConfigPath, cb) {
  if (typeof cb === 'undefined') {
    cb = gitConfigPath;
    gitConfigPath = path.join(
      process.env.HOME || process.env.USERPROFILE, '.gitconfig');
  }
  fs.readFile(gitConfigPath, 'utf-8', function (err, iniContent) {
    if (err) {
      return cb(err)
    }
    cb(null, ini.parse(iniContent))
  })
};

module.exports.sync = function (gitConfigPath) {
  if (typeof gitConfigPath === 'undefined') {
    gitConfigPath = path.join(
      process.env.HOME || process.env.USERPROFILE, '.gitconfig');
  }
  var results = {};
  try {
    results = ini.parse(fs.readFileSync(gitConfigPath, 'utf-8'));
  } catch (err) { }
  return results;
};
