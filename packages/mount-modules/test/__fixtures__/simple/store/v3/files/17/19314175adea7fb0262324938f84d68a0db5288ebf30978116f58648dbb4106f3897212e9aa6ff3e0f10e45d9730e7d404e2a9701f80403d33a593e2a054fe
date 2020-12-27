# git-config

A simple way to extract out all the contents of a .gitconfig file and return as JSON

[![build status](https://secure.travis-ci.org/zkochan/git-config.png)](http://travis-ci.org/zkochan/git-config)

## Installation

This module is installed via npm:

``` bash
$ npm install git-config
```

## Example Usage

### Asynchronous

``` js
var gitConfig = require('git-config');
gitConfig(function (err, config) {
  if (err) return done(err);
  expect(config.user.name).to.equal('Eugene Ware');
  expect(config.user.email).to.equal('eugene@noblesamurai.com');
  expect(config.github.user).to.equal('eugeneware');
  done();
});
```

Explicitly give a gitconfig file:

``` js
var gitConfig = require('git-config');
gitConfig('/my/path/.gitconfig1', function (err, config) {
  if (err) return done(err);
  expect(config.user.name).to.equal('Eugene Ware');
  expect(config.user.email).to.equal('eugene@noblesamurai.com');
  expect(config.github.user).to.equal('eugeneware');
  done();
});
```

### Synchronous

``` js
var gitConfig = require('git-config');
var config = gitConfig.sync(); // can pass explit file if you want as well
```
