const path = require('path')
const findPkgs = require('find-packages')

findPkgs(path.join(__dirname, 'test', 'fixture'))
  .then(pkgs => console.log(pkgs))
  .catch(err => console.error(err))
