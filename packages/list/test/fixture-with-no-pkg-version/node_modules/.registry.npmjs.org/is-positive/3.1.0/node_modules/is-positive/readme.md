# is-positive [![Build Status](https://travis-ci.org/kevva/is-positive.svg?branch=master)](https://travis-ci.org/kevva/is-positive)

> Check if something is a positive number


## Install

```
$ npm install --save is-positive
```


## Usage

```js
const isPositive = require('is-positive');

isPositive(1);
//=> true

isPositive(0);
//=> false

isPositive(-1);
//=> false

isPositive('1');
//=> false

isPositive(Number(1))
//=> true
```

_Note: This module doesn't consider `0` to be a positive number and doesn't distinguish between `-0` and `0`. If you want to detect `0`, use the [`positive-zero`](https://github.com/sindresorhus/positive-zero) module._


## Related

- [is-negative](https://github.com/kevva/is-negative) - Check if something is a negative number


## License

MIT © [Kevin Martensson](http://github.com/kevva)
