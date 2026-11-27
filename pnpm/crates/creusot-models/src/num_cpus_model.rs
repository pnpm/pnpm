//! [`num_cpus::get`] counts at least one CPU.
//!
//! The crate documents the bound, and its code keeps it: on Linux the
//! cgroups quota counts only when positive, the fallback counts the CPUs in
//! the thread's affinity set, which the kernel never leaves empty, and the
//! last fallback raises a count below 1 to 1. The test runs it on one
//! machine only, so the bound rests on that reading more than on the test.

use creusot_std::prelude::{
    ensures,
    extern_spec,
};

extern_spec! {
    mod num_cpus {
        #[ensures(result@ >= 1)]
        fn get() -> usize;
    }
}
