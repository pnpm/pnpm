//! [`pipe_trait::Pipe`]: `value.pipe(function)` is `function(value)`.

use creusot_std::{
    prelude::{
        ensures,
        extern_spec,
        requires,
    },
    std::ops::FnOnceExt,
};
use pipe_trait::Pipe;

extern_spec! {
    mod pipe_trait {
        trait Pipe {
            #[requires(f.precondition((self,)))]
            #[ensures(f.postcondition_once((self,), result))]
            fn pipe<Return, Function>(self, f: Function) -> Return
            where
                Self: Sized,
                Function: FnOnce(Self) -> Return;
        }
    }
}
