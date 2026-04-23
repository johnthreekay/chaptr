//! Compile-time lookup tables for known scanlation groups, LN publishers, and LN scanners.
//!
//! Tables live as static slices for now. They will graduate to `phf::Map` once entry
//! counts exceed ~20 — until then a linear scan over a small `&'static [(&str, T)]`
//! is faster than the PHF hash dispatch and avoids pulling in the dep before the
//! corpus pass tells us what scale we're operating at.

pub mod ln_publishers;
pub mod ln_scanners;
pub mod scanlator_groups;
