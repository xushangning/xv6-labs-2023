#![allow(non_camel_case_types)]
#![allow(unused)]
#![allow(non_upper_case_globals)]
// xv6's C declarations of memcmp/memmove/memset/strlen use `uint` instead of
// `size_t`, which trips this lint since the signatures don't match what the
// standard library expects of these runtime symbols.
#![allow(suspicious_runtime_symbol_definitions)]

include!(concat!(env!("OUT_DIR"), "/bindings.rs"));
