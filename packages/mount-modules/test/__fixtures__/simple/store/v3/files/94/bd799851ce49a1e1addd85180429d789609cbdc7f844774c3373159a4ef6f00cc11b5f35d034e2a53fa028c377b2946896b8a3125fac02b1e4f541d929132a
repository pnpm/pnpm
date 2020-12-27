var expect = require('expect.js'),
    fs = require('fs'),
    path = require('path'),
    gitConfig = require('..');

function fixture(name) {
  return fs.readFileSync(fixturePath(name), { encoding: 'utf8' });
}

function fixturePath(name) {
  return path.join(__dirname, 'fixtures', name);
}

describe('git-config', function() {
  it('should be able to parse a .gitconfig file', function(done) {
    gitConfig(fixturePath('gitconfig1.ini'), function (err, config) {
      if (err) return done(err);
      expect(config.user.name).to.equal('Eugene Ware');
      expect(config.user.email).to.equal('eugene@noblesamurai.com');
      expect(config.github.user).to.equal('eugeneware');
      done();
    });
  });

  it('should be able to look for .gitconfig in the usual places', function(done) {
    process.env.HOME = fixturePath('');
    gitConfig(function (err, config) {
      if (err) return done(err);
      expect(config.user.name).to.equal('Eugene Ware');
      expect(config.user.email).to.equal('eugene@noblesamurai.com');
      expect(config.github.user).to.equal('eugeneware');
      done();
    });
  });

  it('should be able to parse synchronously', function(done) {
    process.env.HOME = fixturePath('');

    var config = gitConfig.sync();
    expect(config.user.name).to.equal('Eugene Ware');
    expect(config.user.email).to.equal('eugene@noblesamurai.com');
    expect(config.github.user).to.equal('eugeneware');

    var config2 = gitConfig.sync(fixturePath('gitconfig1.ini'));
    expect(config2.user.name).to.equal('Eugene Ware');
    expect(config2.user.email).to.equal('eugene@noblesamurai.com');
    expect(config2.github.user).to.equal('eugeneware');
    done();
  });

  it('should be able to pass in a config path synchronously', function(done) {
    var config = gitConfig.sync(fixturePath('gitconfig2.ini'));
    expect(config.user.name).to.equal('Fred Flintstone');
    expect(config.user.email).to.equal('fred@flintstone.com');
    expect(config.github.user).to.equal('fredflintstone');
    done();
  });

  it('should be able to pass in a config path asynchronously', function(done) {
    gitConfig(fixturePath('gitconfig2.ini'), function (err, config) {
      if (err) return done(err);
      expect(config.user.name).to.equal('Fred Flintstone');
      expect(config.user.email).to.equal('fred@flintstone.com');
      expect(config.github.user).to.equal('fredflintstone');
      done();
    });
  });

  it('should be able to get the origin URL', function(done) {
    gitConfig(fixturePath('gitconfig3.ini'), function (err, config) {
      expect(config['remote "origin"'].url).to.equal(
        'git@github.com:eugeneware/git-config.git');
      if (err) return done(err);
      done();
    });
  });

  it('should be able to parse a .gitconfig file with duplicate user sections', function(done) {
    gitConfig(fixturePath('gitconfig4.ini'), function (err, config) {
      if (err) return done(err);
      expect(config.user.name).to.equal('Eugene Ware');
      expect(config.user.email).to.equal('eugene@noblesamurai.com');
      expect(config.github.user).to.equal('eugeneware');
      done();
    });
  });
});
