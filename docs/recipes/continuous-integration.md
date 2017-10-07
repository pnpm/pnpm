# Continuous Integration

pnpm can easily be used in various continuous integration systems.

## Travis

On [Travis CI](https://travis-ci.org/), you can use pnpm for installing your dependencies by adding this to your `.travis.yml` file:

```yaml
before_install:
  - curl -L https://unpkg.com/@pnpm/self-installer | node
install:
  - pnpm install
```

## AppVeyor

On [AppVeyor](https://www.appveyor.com/), you can use pnpm for installing your dependencies by adding this to your `appveyor.yml`:

```yaml
install:
  - ps: Install-Product node $env:nodejs_version
  - curl -L https://unpkg.com/@pnpm/self-installer | node
  - pnpm install
```
