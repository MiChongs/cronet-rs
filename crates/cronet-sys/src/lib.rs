//! Raw bindings generated from the selected Cronet SDK.
//!
//! Every C item with a `Cronet_` prefix is included. ABI ownership and
//! threading requirements are exactly those documented by the SDK header.

#![no_std]
#![allow(non_camel_case_types, non_snake_case, non_upper_case_globals)]
#![allow(
    missing_docs,
    clippy::missing_safety_doc,
    clippy::undocumented_unsafe_blocks
)]

include!(concat!(env!("OUT_DIR"), "/bindings.rs"));
