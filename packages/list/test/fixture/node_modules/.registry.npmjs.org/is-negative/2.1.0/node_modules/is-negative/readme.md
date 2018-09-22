# is-negative [![Build Status](https://travis-ci.org/kevva/is-negative.svg?branch=master)](https://travis-ci.org/kevva/is-negative)

> Check if something is a negative number


## Install

```
$ npm install --save is-negative
```


## Usage

```js
const isNegative = require('is-negative');

isNegative(-1);
//=> true

isNegative(1);
//=> false

isNegative(0);
//=> false

isNegative('-1');
//=> false

isNegative(Number(-1))
//=> true
```

_Note: This module doesn't consider `-0` to be a negative number. If you want to detect `-0`, use the [`negative-zero`](https://github.com/sindresorhus/negative-zero) module._


## Related

- [is-positive](https://github.com/kevva/is-positive) - Check if something is a positive number


## License

MIT © [Kevin Martensson](http://github.com/kevva)
