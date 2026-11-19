//! Compatibility shim for the M0 Annex B helper.
//!
//! The normalizer was promoted to [`crate::media::h264`] in M1; this module only
//! re-exports it so the old `replay::annex_b` path keeps resolving. New code
//! must import `lotu_adapters::media::h264` directly.

pub use crate::media::h264::{
    annex_b_to_avcc, avcc_from_annex_b, build_avcc, contains_idr, convert_annex_b, is_idr,
    nal_type, normalize_sample, parameter_sets, split_annex_b, AnnexBClip, AvccSample, H264Error,
    ParameterSets,
};
