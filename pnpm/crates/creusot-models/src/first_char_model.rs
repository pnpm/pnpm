//! [`split_first_char`]: the first character of a string and the rest.

use creusot_std::prelude::{
    Seq,
    ensures,
    extern_spec,
};

extern_spec! {
    mod split_first_char {
        #[ensures(match result {
            None => text@.len() == 0,
            Some((first, rest)) => text@ == Seq::singleton(first).concat(rest@),
        })]
        fn split_first_char(text: &str) -> Option<(char, &str)>;
    }

    impl<'a> split_first_char::SplitFirstChar<'a> for &'a str {
        #[ensures(match result {
            None => self@.len() == 0,
            Some((first, rest)) => self@ == Seq::singleton(first).concat(rest@),
        })]
        fn split_first_char(self) -> Option<(char, &'a str)>;
    }
}
