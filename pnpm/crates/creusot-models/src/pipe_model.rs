//! [`pipe_trait::Pipe`]: each method applies `f` to the receiver, or to
//! what `as_ref` or `deref` returns for it.

use core::ops::Deref;
use creusot_std::{
    prelude::{
        ensures,
        extern_spec,
        requires,
    },
    std::ops::{
        FnExt,
        FnOnceExt,
    },
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

            #[requires(f.precondition((self,)))]
            #[ensures(f.postcondition_once((self,), result))]
            fn pipe_ref<'a, Return, Function>(&'a self, f: Function) -> Return
            where
                Function: FnOnce(&'a Self) -> Return;

            #[requires(f.precondition((self,)))]
            #[ensures(f.postcondition_once((self,), result))]
            fn pipe_mut<'a, Return, Function>(&'a mut self, f: Function) -> Return
            where
                Function: FnOnce(&'a mut Self) -> Return;

            #[requires(<Self as AsRef<Param>>::as_ref.precondition((self,)))]
            #[requires(forall<param: &'a Param>
                <Self as AsRef<Param>>::as_ref.postcondition((self,), param)
                    ==> f.precondition((param,)))]
            #[ensures(exists<param: &'a Param>
                <Self as AsRef<Param>>::as_ref.postcondition((self,), param)
                    && f.postcondition_once((param,), result))]
            fn pipe_as_ref<'a, Param, Return, Function>(&'a self, f: Function) -> Return
            where
                Self: AsRef<Param>,
                Param: ?Sized + 'a,
                Function: FnOnce(&'a Param) -> Return;

            #[requires(<Self as Deref>::deref.precondition((self,)))]
            #[requires(forall<param: &'a Param>
                <Self as Deref>::deref.postcondition((self,), param)
                    ==> f.precondition((param,)))]
            #[ensures(exists<param: &'a Param>
                <Self as Deref>::deref.postcondition((self,), param)
                    && f.postcondition_once((param,), result))]
            fn pipe_deref<'a, Param, Return, Function>(&'a self, f: Function) -> Return
            where
                Self: Deref<Target = Param>,
                Param: ?Sized + 'a,
                Function: FnOnce(&'a Param) -> Return;
        }
    }
}
