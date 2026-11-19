//! H.264 Annex B → AVCC normalization.
//!
//! The scrcpy server streams H.264 in Annex B framing (`00 00 01` /
//! `00 00 00 01` start codes). The internal stream described in
//! `docs/stream-protocol.md` and WebCodecs expect AVCC: every NAL unit prefixed
//! with its length as a 4-byte big-endian integer, and the SPS/PPS carried by
//! an `avcC` decoder configuration record instead of in-band samples.
//!
//! Entry points:
//!
//! - [`normalize_sample`] converts one access unit (one scrcpy video packet)
//!   and reports whether it contains an IDR slice.
//! - [`avcc_from_annex_b`] builds the `avcC` record from a scrcpy configuration
//!   packet (SPS/PPS in Annex B).
//! - [`convert_annex_b`] is the offline whole-clip path used by the
//!   `make_replay_fixture` example.

/// NAL unit type of a codified slice of a non-IDR picture (types 1-4 are
/// partition/extension variants of the same group).
const NAL_TYPE_SLICE: u8 = 1;
/// NAL unit type of a codified slice of an IDR picture.
const NAL_TYPE_IDR: u8 = 5;
const NAL_TYPE_SEI: u8 = 6;
const NAL_TYPE_SPS: u8 = 7;
const NAL_TYPE_PPS: u8 = 8;
const NAL_TYPE_AUD: u8 = 9;

/// Errors raised while normalizing H.264 data.
///
/// The M1 session adapter maps these to `SessionError::Failed`; they always mean
/// corrupt or unexpected data, never a retryable condition.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum H264Error {
    #[error("input contains no Annex B NAL units")]
    NoNalUnits,
    #[error("Annex B input ends with a start code but no NAL unit")]
    TruncatedNal,
    #[error("input is missing an SPS NAL unit")]
    MissingSps,
    #[error("input is missing a PPS NAL unit")]
    MissingPps,
    #[error("input contains no H.264 video access units")]
    NoFrames,
    #[error("invalid SPS: {0}")]
    InvalidSps(String),
    #[error("invalid PPS: {0}")]
    InvalidPps(String),
    #[error("NAL unit is larger than the 32-bit AVCC length prefix allows")]
    TooLarge,
}

/// SPS and PPS extracted from an Annex B buffer, without start codes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParameterSets {
    pub sps: Vec<u8>,
    pub pps: Vec<u8>,
}

/// One access unit already encoded in AVCC (4-byte big-endian NAL lengths).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AvccSample {
    pub data: Vec<u8>,
    /// True when the sample contains an IDR slice (NAL type 5).
    pub keyframe: bool,
}

/// Result of converting a complete Annex B clip.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AnnexBClip {
    /// `avcC` record built from the first SPS/PPS of the clip.
    pub avcc: Vec<u8>,
    /// Video access units; SPS/PPS are excluded because `avcc` carries them.
    pub samples: Vec<AvccSample>,
}

/// NAL unit type (the low 5 bits of the NAL header), or `None` if empty.
pub fn nal_type(nal: &[u8]) -> Option<u8> {
    nal.first().map(|header| header & 0x1F)
}

/// Whether this NAL unit is an IDR slice.
pub fn is_idr(nal: &[u8]) -> bool {
    nal_type(nal) == Some(NAL_TYPE_IDR)
}

/// Splits an Annex B buffer into NAL units without start codes.
///
/// Both 3-byte (`00 00 01`) and 4-byte (`00 00 00 01`) start codes are
/// accepted. Trailing zero bytes belong to the next start code or to Annex B
/// padding and are removed from the NAL. Leading bytes before the first start
/// code are ignored, as allowed by the Annex B byte stream syntax.
pub fn split_annex_b(data: &[u8]) -> Result<Vec<&[u8]>, H264Error> {
    let mut nals = Vec::new();
    let mut start: Option<usize> = None;
    let mut index = 0;

    while index + 2 < data.len() {
        if data[index] == 0 && data[index + 1] == 0 && data[index + 2] == 1 {
            if let Some(begin) = start {
                push_nal(&mut nals, &data[begin..index]);
            }
            index += 3;
            start = Some(index);
        } else {
            index += 1;
        }
    }

    match start {
        None => return Err(H264Error::NoNalUnits),
        Some(begin) => {
            let nal = trim_trailing_zeros(&data[begin..]);
            if nal.is_empty() {
                return Err(H264Error::TruncatedNal);
            }
            nals.push(nal);
        }
    }

    if nals.is_empty() {
        return Err(H264Error::NoNalUnits);
    }

    Ok(nals)
}

/// Whether any NAL unit of the Annex B buffer is an IDR slice.
///
/// The scrcpy protocol keyframe flag is authoritative (bit 61); this check is
/// the cross-check preserved for tests.
pub fn contains_idr(data: &[u8]) -> Result<bool, H264Error> {
    Ok(split_annex_b(data)?.iter().any(|nal| is_idr(nal)))
}

/// Encodes NAL units in AVCC: 4-byte big-endian length followed by the NAL.
pub fn encode_nal_units<'a, I>(nals: I) -> Result<Vec<u8>, H264Error>
where
    I: IntoIterator<Item = &'a [u8]>,
{
    let mut data = Vec::new();
    for nal in nals {
        let length = u32::try_from(nal.len()).map_err(|_| H264Error::TooLarge)?;
        data.extend_from_slice(&length.to_be_bytes());
        data.extend_from_slice(nal);
    }
    Ok(data)
}

/// Converts one Annex B access unit into AVCC.
pub fn annex_b_to_avcc(data: &[u8]) -> Result<Vec<u8>, H264Error> {
    let nals = split_annex_b(data)?;
    encode_nal_units(nals.iter().copied())
}

/// Converts one Annex B access unit into AVCC and reports whether it is an IDR.
pub fn normalize_sample(data: &[u8]) -> Result<AvccSample, H264Error> {
    let nals = split_annex_b(data)?;
    let keyframe = nals.iter().any(|nal| is_idr(nal));
    let data = encode_nal_units(nals.iter().copied())?;
    Ok(AvccSample { data, keyframe })
}

/// Extracts the first SPS and the first PPS of an Annex B buffer.
pub fn parameter_sets(data: &[u8]) -> Result<ParameterSets, H264Error> {
    let nals = split_annex_b(data)?;
    collect_parameter_sets(&nals)
}

/// Builds an `avcC` record from one SPS and one PPS (`lengthSizeMinusOne = 3`).
///
/// The NAL units must not contain Annex B start codes; [`parameter_sets`]
/// returns them already stripped.
pub fn build_avcc(sps: &[u8], pps: &[u8]) -> Result<Vec<u8>, H264Error> {
    if sps.len() < 4 {
        return Err(H264Error::InvalidSps(
            "SPS must contain profile, compatibility and level bytes".to_owned(),
        ));
    }
    if nal_type(sps) != Some(NAL_TYPE_SPS) {
        return Err(H264Error::InvalidSps(format!(
            "expected NAL type {NAL_TYPE_SPS}, got {:?}",
            nal_type(sps)
        )));
    }
    if pps.is_empty() {
        return Err(H264Error::InvalidPps("PPS must not be empty".to_owned()));
    }
    if nal_type(pps) != Some(NAL_TYPE_PPS) {
        return Err(H264Error::InvalidPps(format!(
            "expected NAL type {NAL_TYPE_PPS}, got {:?}",
            nal_type(pps)
        )));
    }

    let sps_len = u16::try_from(sps.len())
        .map_err(|_| H264Error::InvalidSps("SPS is larger than 65535 bytes".to_owned()))?;
    let pps_len = u16::try_from(pps.len())
        .map_err(|_| H264Error::InvalidPps("PPS is larger than 65535 bytes".to_owned()))?;

    let mut avcc = Vec::with_capacity(11 + sps.len() + pps.len());
    avcc.push(1);
    avcc.extend_from_slice(&sps[1..4]);
    avcc.push(0xFF); // reserved 6 bits + lengthSizeMinusOne = 3
    avcc.push(0xE1); // reserved 3 bits + numOfSPS = 1
    avcc.extend_from_slice(&sps_len.to_be_bytes());
    avcc.extend_from_slice(sps);
    avcc.push(1);
    avcc.extend_from_slice(&pps_len.to_be_bytes());
    avcc.extend_from_slice(pps);

    Ok(avcc)
}

/// Builds the `avcC` record for an Annex B configuration buffer (SPS/PPS).
pub fn avcc_from_annex_b(data: &[u8]) -> Result<Vec<u8>, H264Error> {
    let sets = parameter_sets(data)?;
    build_avcc(&sets.sps, &sets.pps)
}

/// Converts a complete Annex B clip into `avcC` plus AVCC access units.
///
/// Used by the offline `make_replay_fixture` tool: PTS assignment and frame
/// pacing are fixture policies and stay out of this module.
pub fn convert_annex_b(data: &[u8]) -> Result<AnnexBClip, H264Error> {
    let nals = split_annex_b(data)?;
    let sets = collect_parameter_sets(&nals)?;
    let units = group_access_units(&nals);
    if units.is_empty() {
        return Err(H264Error::NoFrames);
    }

    let avcc = build_avcc(&sets.sps, &sets.pps)?;
    let mut samples = Vec::with_capacity(units.len());
    for unit in units {
        let data = encode_nal_units(unit.nals.iter().copied())?;
        samples.push(AvccSample {
            data,
            keyframe: unit.keyframe,
        });
    }

    Ok(AnnexBClip { avcc, samples })
}

fn collect_parameter_sets(nals: &[&[u8]]) -> Result<ParameterSets, H264Error> {
    let mut sps: Option<Vec<u8>> = None;
    let mut pps: Option<Vec<u8>> = None;

    for nal in nals {
        match nal_type(nal) {
            Some(NAL_TYPE_SPS) if sps.is_none() => sps = Some((*nal).to_vec()),
            Some(NAL_TYPE_PPS) if pps.is_none() => pps = Some((*nal).to_vec()),
            _ => {}
        }
    }

    let sps = sps.ok_or(H264Error::MissingSps)?;
    let pps = pps.ok_or(H264Error::MissingPps)?;

    Ok(ParameterSets { sps, pps })
}

#[derive(Default)]
struct AccessUnit<'a> {
    nals: Vec<&'a [u8]>,
    has_slice: bool,
    keyframe: bool,
}

/// Groups NAL units into video access units, dropping parameter sets.
fn group_access_units<'a>(nals: &[&'a [u8]]) -> Vec<AccessUnit<'a>> {
    let mut units: Vec<AccessUnit<'a>> = Vec::new();
    let mut current = AccessUnit::default();

    for &nal in nals {
        match nal_type(nal) {
            // Access unit delimiter: starts a new access unit.
            Some(NAL_TYPE_AUD) => flush(&mut units, &mut current),
            // Parameter sets travel in `avcC`, never inside a sample.
            Some(NAL_TYPE_SPS | NAL_TYPE_PPS) => {
                if current.has_slice {
                    flush(&mut units, &mut current);
                }
            }
            // Supplemental enhancement information belongs to the next picture.
            Some(NAL_TYPE_SEI) => {
                if current.has_slice {
                    flush(&mut units, &mut current);
                }
                current.nals.push(nal);
            }
            // Coded slice of a non-IDR, data partition, or IDR picture.
            Some(slice @ NAL_TYPE_SLICE..=NAL_TYPE_IDR) => {
                let first_mb = first_mb_in_slice(nal).is_none_or(|value| value == 0);
                if current.has_slice && first_mb {
                    flush(&mut units, &mut current);
                }
                current.has_slice = true;
                current.keyframe |= slice == NAL_TYPE_IDR;
                current.nals.push(nal);
            }
            _ => current.nals.push(nal),
        }
    }
    flush(&mut units, &mut current);

    units
}

fn flush<'a>(units: &mut Vec<AccessUnit<'a>>, current: &mut AccessUnit<'a>) {
    if current.has_slice && !current.nals.is_empty() {
        units.push(std::mem::take(current));
    } else {
        current.nals.clear();
        current.has_slice = false;
        current.keyframe = false;
    }
}

fn push_nal<'a>(nals: &mut Vec<&'a [u8]>, nal: &'a [u8]) {
    let nal = trim_trailing_zeros(nal);
    if !nal.is_empty() {
        nals.push(nal);
    }
}

fn trim_trailing_zeros(nal: &[u8]) -> &[u8] {
    let end = nal
        .iter()
        .rposition(|&byte| byte != 0)
        .map_or(0, |position| position + 1);
    &nal[..end]
}

/// Reads `first_mb_in_slice` (ue(v)) from a slice NAL unit.
///
/// Returns `None` when the header cannot be parsed; callers treat that as the
/// start of a new access unit, which is the safe choice for single-slice
/// streams.
fn first_mb_in_slice(nal: &[u8]) -> Option<u32> {
    let rbsp = unescape_prefix(nal.get(1..)?, 8);
    let mut reader = BitReader::new(&rbsp);
    reader.read_ue()
}

fn unescape_prefix(bytes: &[u8], limit: usize) -> Vec<u8> {
    let mut out = Vec::with_capacity(limit);
    let mut zeros = 0u8;

    for &byte in bytes {
        if zeros >= 2 && byte == 3 {
            zeros = 0;
            continue;
        }
        out.push(byte);
        zeros = if byte == 0 {
            zeros.saturating_add(1)
        } else {
            0
        };
        if out.len() >= limit {
            break;
        }
    }

    out
}

struct BitReader<'a> {
    data: &'a [u8],
    bit: usize,
}

impl<'a> BitReader<'a> {
    fn new(data: &'a [u8]) -> Self {
        Self { data, bit: 0 }
    }

    fn read_bit(&mut self) -> Option<u32> {
        let byte = *self.data.get(self.bit / 8)?;
        let shift = 7 - (self.bit % 8);
        self.bit += 1;
        Some(u32::from((byte >> shift) & 1))
    }

    fn read_ue(&mut self) -> Option<u32> {
        let mut leading_zeros = 0u32;
        while let 0 = self.read_bit()? {
            leading_zeros += 1;
            if leading_zeros > 31 {
                return None;
            }
        }

        let mut value = 0u32;
        for _ in 0..leading_zeros {
            value = (value << 1) | self.read_bit()?;
        }

        Some((1u32 << leading_zeros) - 1 + value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SPS: [u8; 4] = [0x67, 0x42, 0xC0, 0x1F];
    const PPS: [u8; 4] = [0x68, 0xCE, 0x3C, 0x80];
    const IDR: [u8; 4] = [0x65, 0x88, 0x84, 0x21];
    const P_FRAME: [u8; 4] = [0x41, 0x9A, 0x22, 0x33];

    fn annex_b(nal_units: &[&[u8]], four_byte_start_code: bool) -> Vec<u8> {
        let mut out = Vec::new();
        for nal in nal_units {
            if four_byte_start_code {
                out.extend_from_slice(&[0, 0, 0, 1]);
            } else {
                out.extend_from_slice(&[0, 0, 1]);
            }
            out.extend_from_slice(nal);
        }
        out
    }

    #[test]
    fn splits_three_and_four_byte_start_codes() {
        let stream = annex_b(&[&SPS, &PPS, &IDR], true);
        assert_eq!(
            split_annex_b(&stream).expect("four-byte start codes should split"),
            vec![&SPS[..], &PPS[..], &IDR[..]]
        );

        let stream = annex_b(&[&SPS, &PPS, &IDR], false);
        assert_eq!(
            split_annex_b(&stream).expect("three-byte start codes should split"),
            vec![&SPS[..], &PPS[..], &IDR[..]]
        );
    }

    #[test]
    fn trims_annex_b_padding_after_the_last_nal() {
        let mut stream = annex_b(&[&SPS], false);
        stream.extend_from_slice(&[0, 0, 0]);

        assert_eq!(
            split_annex_b(&stream).expect("trailing zero bytes are padding"),
            vec![&SPS[..]]
        );
    }

    #[test]
    fn rejects_input_without_start_codes() {
        assert_eq!(split_annex_b(&[]), Err(H264Error::NoNalUnits));
        assert_eq!(
            split_annex_b(&[0x65, 0x88, 0x84]),
            Err(H264Error::NoNalUnits)
        );
    }

    #[test]
    fn rejects_start_code_without_nal_unit() {
        assert_eq!(split_annex_b(&[0, 0, 1]), Err(H264Error::TruncatedNal));
        assert_eq!(
            split_annex_b(&[0, 0, 0, 1, 0, 0, 1]),
            Err(H264Error::TruncatedNal)
        );

        let mut stream = annex_b(&[&SPS], true);
        stream.extend_from_slice(&[0, 0, 0, 1]);
        assert_eq!(split_annex_b(&stream), Err(H264Error::TruncatedNal));
    }

    #[test]
    fn normalizes_sample_with_four_byte_lengths() {
        let sample =
            normalize_sample(&annex_b(&[&IDR, &P_FRAME], true)).expect("sample should normalize");

        assert!(sample.keyframe);
        assert_eq!(
            sample.data,
            [0u8, 0, 0, 4, 0x65, 0x88, 0x84, 0x21, 0, 0, 0, 4, 0x41, 0x9A, 0x22, 0x33,]
        );
        assert_eq!(
            annex_b_to_avcc(&annex_b(&[&P_FRAME], false)).expect("sample should convert"),
            [0u8, 0, 0, 4, 0x41, 0x9A, 0x22, 0x33]
        );
    }

    #[test]
    fn detects_idr_nal_units() {
        assert!(is_idr(&IDR));
        assert!(!is_idr(&P_FRAME));
        assert!(!is_idr(&[]));

        assert!(contains_idr(&annex_b(&[&P_FRAME, &IDR], false)).expect("buffer should split"));
        assert!(!contains_idr(&annex_b(&[&P_FRAME], false)).expect("buffer should split"));
    }

    #[test]
    fn extracts_first_sps_and_pps() {
        let second_sps = [0x67, 0x4D, 0x00, 0x1F];
        let stream = annex_b(&[&SPS, &PPS, &second_sps, &PPS, &IDR], true);
        let sets = parameter_sets(&stream).expect("parameter sets should be found");

        assert_eq!(sets.sps, SPS);
        assert_eq!(sets.pps, PPS);
    }

    #[test]
    fn rejects_missing_parameter_sets() {
        assert_eq!(
            parameter_sets(&annex_b(&[&IDR, &P_FRAME], true)),
            Err(H264Error::MissingSps)
        );
        assert_eq!(
            parameter_sets(&annex_b(&[&SPS, &IDR], true)),
            Err(H264Error::MissingPps)
        );
    }

    #[test]
    fn builds_avcc_from_sps_and_pps() {
        let avcc = build_avcc(&SPS, &PPS).expect("avcC should be built");

        assert_eq!(avcc[0], 1);
        assert_eq!(&avcc[1..4], &SPS[1..4]);
        assert_eq!(avcc[4], 0xFF);
        assert_eq!(avcc[5] & 0x1F, 1);
        assert_eq!(u16::from_be_bytes([avcc[6], avcc[7]]), 4);
        assert_eq!(&avcc[8..12], &SPS);
        assert_eq!(avcc[12], 1);
        assert_eq!(u16::from_be_bytes([avcc[13], avcc[14]]), 4);
        assert_eq!(&avcc[15..19], &PPS);

        assert_eq!(
            avcc_from_annex_b(&annex_b(&[&SPS, &PPS, &IDR], true)).expect("avcC"),
            avcc
        );
    }

    #[test]
    fn rejects_invalid_parameter_sets() {
        assert!(matches!(
            build_avcc(&[0x67, 0x42], &PPS),
            Err(H264Error::InvalidSps(_))
        ));
        assert!(matches!(
            build_avcc(&[0x68, 0xCE, 0x3C, 0x80], &PPS),
            Err(H264Error::InvalidSps(_))
        ));
        assert!(matches!(
            build_avcc(&SPS, &[]),
            Err(H264Error::InvalidPps(_))
        ));
        assert!(matches!(
            build_avcc(&SPS, &SPS),
            Err(H264Error::InvalidPps(_))
        ));
    }

    #[test]
    fn converts_clip_into_avcc_samples() {
        let clip = convert_annex_b(&annex_b(&[&SPS, &PPS, &IDR, &P_FRAME], true)).expect("clip");

        assert_eq!(clip.samples.len(), 2);
        assert!(clip.samples[0].keyframe);
        assert_eq!(clip.samples[0].data, [0u8, 0, 0, 4, 0x65, 0x88, 0x84, 0x21]);
        assert!(!clip.samples[1].keyframe);
        assert_eq!(clip.samples[1].data, [0u8, 0, 0, 4, 0x41, 0x9A, 0x22, 0x33]);
        assert_eq!(clip.avcc, build_avcc(&SPS, &PPS).expect("avcC"));
    }

    #[test]
    fn groups_multiple_slices_of_one_frame() {
        let second_slice = [0x65, 0x10, 0x00, 0x11];
        let clip = convert_annex_b(&annex_b(&[&SPS, &PPS, &IDR, &second_slice, &P_FRAME], true))
            .expect("clip");

        assert_eq!(clip.samples.len(), 2);
        assert!(clip.samples[0].keyframe);
        assert_eq!(clip.samples[0].data.len(), 16);
    }

    #[test]
    fn starts_new_access_unit_at_first_mb_zero() {
        let new_picture = [0x65, 0x80, 0x00, 0x11];
        let clip =
            convert_annex_b(&annex_b(&[&SPS, &PPS, &IDR, &new_picture], true)).expect("clip");

        assert_eq!(clip.samples.len(), 2);
        assert!(clip.samples[0].keyframe);
        assert!(clip.samples[1].keyframe);
    }

    #[test]
    fn rejects_clip_without_parameter_sets() {
        assert_eq!(
            convert_annex_b(&annex_b(&[&IDR, &P_FRAME], true)),
            Err(H264Error::MissingSps)
        );
    }

    #[test]
    fn rejects_clip_without_frames() {
        assert_eq!(
            convert_annex_b(&annex_b(&[&SPS, &PPS], true)),
            Err(H264Error::NoFrames)
        );
    }
}
