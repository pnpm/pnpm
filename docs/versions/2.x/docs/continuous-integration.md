---
id: continuous-integration
title: Continuous Integration
original_id: continuous-integration
---

pnpm can easily be used in various continuous integration systems.

## Travis

On [Travis CI](https://travis-ci.org/), you can use pnpm for installing your dependencies by adding this to your `.travis.yml` file:

```yaml
before_install:
  - curl -L https://unpkg.com/@pnpm/self-installer | node
install:
  - pnpm install
```

## Semaphore

On [Semaphore](https://semaphoreci.com), you can use pnpm for installing and caching your dependencies by adding this to your `.semaphore/semaphore.yml` file:

```yaml
version: v1.0
name: Semaphore CI pnpm example
agent:
  machine:
    type: e1-standard-2
    os_image: ubuntu1804
blocks:
  - name: Install dependencies
    task:
      jobs:
        - name: pnpm install
          commands:
            - curl -L https://unpkg.com/@pnpm/self-installer | node
            - checkout
            - cache restore node-modules-$SEMAPHORE_GIT_BRANCH-$(checksum package-lock.json),node-modules-$SEMAPHORE_GIT_BRANCH,node-modules-master
            - pnpm install
            - cache store node-modules-$SEMAPHORE_GIT_BRANCH-$(checksum package-lock.json) node_modules
```

## AppVeyor

On [AppVeyor](https://www.appveyor.com/), you can use pnpm for installing your dependencies by adding this to your `appveyor.yml`:

```yaml
install:
  - ps: Install-Product node $env:nodejs_version
  - curl -L https://unpkg.com/@pnpm/self-installer | node
  - pnpm install
```

## Sail CI

On [Sail CI](https://sail.ci/), you can use pnpm for installing your dependencies by adding this to your `.sail.yml` file:

```yaml
install:
  image: znck/pnpm:latest
  command:
    - pnpm
  args:
    - install
```
To get the exact Node version and pnpm version you require you can always make your own Docker image and push to [Docker Hub](https://hub.docker.com/).
