//! The slice methods the verified crates call.

use creusot_std::prelude::{
    DeepModel,
    ensures,
    extern_spec,
};

extern_spec! {
    impl<T> [T] {
        #[ensures(result == exists<index> 0 <= index && index < self@.len()
            && self.deep_model()[index] == x.deep_model())]
        fn contains(&self, x: &T) -> bool
            where T: PartialEq + DeepModel;
    }
}
