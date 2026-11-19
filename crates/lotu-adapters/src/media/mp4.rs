//! Minimal progressive MP4 muxer for scrcpy's normalized H.264 stream (M3).
//!
//! Samples are copied verbatim: no re-encoding and no audio. The media
//! timescale is 1_000_000 so the `pts_us` values of the stream become exact
//! sample timestamps. Layout: `ftyp`, `mdat`, `moov` (moov at the end, so chunk
//! offsets are known when it is built). One chunk per sample keeps the sample
//! tables simple.
//!
//! scrcpy's stream exposes only PTS, so DTS = PTS is assumed. Its low-latency
//! encoder does not emit B-frames; this is documented as an M3 risk.

use std::io::{self, Seek, SeekFrom, Write};

use lotu_app::ports::{RecordingError, RecordingStats};

/// Media timescale: one tick per microsecond, matching the stream's PTS.
pub const MEDIA_TIMESCALE: u32 = 1_000_000;
/// Movie timescale: milliseconds, used by `mvhd` and `tkhd`.
const MOVIE_TIMESCALE: u32 = 1000;
/// Fallback duration for the last sample when only one frame was recorded.
const DEFAULT_FRAME_US: u64 = 1_000_000 / 60;

#[derive(Debug, Clone)]
struct Sample {
    offset: u64,
    size: u32,
    delta_us: u64,
    keyframe: bool,
}

/// Stateful writer. Callers feed `Config` then `Frame` messages in order.
#[derive(Debug)]
pub struct Mp4Writer<W: Write + Seek> {
    inner: W,
    samples: Vec<Sample>,
    avcc: Vec<u8>,
    width: u32,
    height: u32,
    /// Byte offset of the `mdat` box header; its size field is patched here.
    mdat_offset: u64,
    position: u64,
    last_pts: Option<i64>,
    finished: bool,
}

impl<W: Write + Seek> Mp4Writer<W> {
    /// Writes `ftyp` and an empty `mdat` header, reserving a seed config.
    pub fn new(mut inner: W, seed: Option<&(u32, u32, &[u8])>) -> Result<Self, RecordingError> {
        let mut ftyp = Vec::new();
        ftyp.extend_from_slice(b"isom"); // major brand
        ftyp.extend_from_slice(&0u32.to_be_bytes()); // minor version
        for brand in [b"isom", b"iso2", b"avc1", b"mp41"] {
            ftyp.extend_from_slice(brand);
        }
        let ftyp_bytes = box_bytes(b"ftyp", &ftyp);
        write_all(&mut inner, &ftyp_bytes)?;
        let mdat_offset = ftyp_bytes.len() as u64;

        let mut mdat_header = Vec::with_capacity(8);
        mdat_header.extend_from_slice(&8u32.to_be_bytes());
        mdat_header.extend_from_slice(b"mdat");
        write_all(&mut inner, &mdat_header)?;

        let mut writer = Self {
            inner,
            samples: Vec::new(),
            avcc: Vec::new(),
            width: 0,
            height: 0,
            mdat_offset,
            position: mdat_offset + 8,
            last_pts: None,
            finished: false,
        };
        if let Some((width, height, avcc)) = seed {
            writer.set_config(*width, *height, avcc);
        }
        Ok(writer)
    }

    /// Replaces the decoder configuration. A late config changes the sample
    /// entry for the whole file; scrcpy only reconfigures on rotation, where
    /// restarting the recording is preferable but not enforced here.
    pub fn set_config(&mut self, width: u32, height: u32, avcc: &[u8]) {
        self.width = width;
        self.height = height;
        self.avcc = avcc.to_vec();
    }

    pub fn has_config(&self) -> bool {
        !self.avcc.is_empty()
    }

    /// Appends one access unit and returns its wire size.
    pub fn write_frame(
        &mut self,
        pts_us: i64,
        keyframe: bool,
        data: &[u8],
    ) -> Result<usize, RecordingError> {
        if self.finished {
            return Err(RecordingError::Failed(
                "writer is already finished".to_owned(),
            ));
        }
        if !self.has_config() {
            return Err(RecordingError::Invalid(
                "frame arrived before the decoder configuration".to_owned(),
            ));
        }
        if data.is_empty() {
            return Err(RecordingError::Invalid("empty H.264 sample".to_owned()));
        }
        if self.samples.is_empty() && !keyframe {
            return Err(RecordingError::Invalid(
                "the first recorded sample must be a keyframe".to_owned(),
            ));
        }
        if data.len() > u32::MAX as usize {
            return Err(RecordingError::Invalid(
                "H.264 sample is too large".to_owned(),
            ));
        }

        if let Some(previous_pts) = self.last_pts {
            if let Some(previous) = self.samples.last_mut() {
                // A RESET_VIDEO can restart the PTS origin. Treat backwards
                // or implausible jumps as one normal frame interval instead
                // of writing zero/long durations into `stts`.
                previous.delta_us = pts_us
                    .checked_sub(previous_pts)
                    .filter(|delta| *delta >= 0 && *delta <= 1_000_000)
                    .unwrap_or(DEFAULT_FRAME_US as i64) as u64;
            }
        }

        let offset = self.position;
        write_all(&mut self.inner, data)?;
        self.position += data.len() as u64;
        self.samples.push(Sample {
            offset,
            size: data.len() as u32,
            delta_us: DEFAULT_FRAME_US,
            keyframe,
        });
        self.last_pts = Some(pts_us);
        Ok(data.len())
    }

    /// Patches the `mdat` size and appends the complete `moov` box.
    pub fn finish(&mut self) -> Result<RecordingStats, RecordingError> {
        if self.finished {
            return Err(RecordingError::Failed(
                "writer is already finished".to_owned(),
            ));
        }
        if self.samples.is_empty() {
            return Err(RecordingError::Invalid(
                "no frames were recorded".to_owned(),
            ));
        }
        if !self.has_config() {
            return Err(RecordingError::Invalid(
                "missing decoder configuration".to_owned(),
            ));
        }

        let payload_size = self
            .position
            .checked_sub(self.mdat_offset + 8)
            .ok_or_else(|| RecordingError::Failed("invalid mdat size".to_owned()))?;
        let mdat_size = payload_size
            .checked_add(8)
            .filter(|size| *size <= u32::MAX as u64)
            .ok_or_else(|| RecordingError::Failed("MP4 exceeds the 4 GiB mdat limit".to_owned()))?;

        self.inner
            .seek(SeekFrom::Start(self.mdat_offset))
            .map_err(io_error)?;
        write_all(&mut self.inner, &(mdat_size as u32).to_be_bytes())?;
        self.inner.seek(SeekFrom::End(0)).map_err(io_error)?;

        // The last sample has no successor to derive a delta from: reuse the
        // previous one (or a 60 fps default when there is only one frame).
        let last_delta = if self.samples.len() >= 2 {
            self.samples[self.samples.len() - 2].delta_us
        } else {
            DEFAULT_FRAME_US
        };
        self.samples
            .last_mut()
            .expect("samples are not empty")
            .delta_us = last_delta;

        let duration_us = self.duration_us();
        let moov = self.moov(duration_us)?;
        write_all(&mut self.inner, &moov)?;
        self.inner.flush().map_err(io_error)?;

        self.finished = true;
        Ok(RecordingStats {
            frames: self.samples.len() as u64,
            duration_us,
        })
    }

    fn duration_us(&self) -> u64 {
        self.samples.iter().map(|sample| sample.delta_us).sum()
    }

    fn moov(&self, duration_us: u64) -> Result<Vec<u8>, RecordingError> {
        if duration_us > u32::MAX as u64 {
            return Err(RecordingError::Failed(
                "recording is longer than the MP4 timescale allows".to_owned(),
            ));
        }
        let movie_duration = duration_us.div_ceil(1000).min(u32::MAX as u64) as u32;

        let mut payload = Vec::new();
        payload.extend(mvhd(movie_duration));
        payload.extend(self.trak(movie_duration, duration_us as u32)?);
        Ok(box_bytes(b"moov", &payload))
    }

    fn trak(&self, movie_duration: u32, media_duration: u32) -> Result<Vec<u8>, RecordingError> {
        let mut media = Vec::new();
        media.extend(mdhd(media_duration));
        media.extend(hdlr());
        media.extend(minf(&self.samples, &self.avcc, self.width, self.height)?);

        let mut trak = full_box(
            b"tkhd",
            0,
            0x000007,
            &tkhd(movie_duration, self.width, self.height),
        );
        trak.extend(box_bytes(b"mdia", &media));
        Ok(box_bytes(b"trak", &trak))
    }
}

fn mvhd(movie_duration: u32) -> Vec<u8> {
    let mut payload = Vec::new();
    payload.extend_from_slice(&0u32.to_be_bytes()); // creation
    payload.extend_from_slice(&0u32.to_be_bytes()); // modification
    payload.extend_from_slice(&MOVIE_TIMESCALE.to_be_bytes());
    payload.extend_from_slice(&movie_duration.to_be_bytes());
    payload.extend_from_slice(&0x0001_0000u32.to_be_bytes()); // rate 1.0
    payload.extend_from_slice(&0x0100u16.to_be_bytes()); // volume 1.0
    payload.extend_from_slice(&0u16.to_be_bytes()); // reserved
    payload.extend_from_slice(&0u32.to_be_bytes());
    payload.extend_from_slice(&0u32.to_be_bytes());
    payload.extend_from_slice(&identity_matrix());
    for _ in 0..6 {
        payload.extend_from_slice(&0u32.to_be_bytes()); // pre_defined
    }
    payload.extend_from_slice(&2u32.to_be_bytes()); // next_track_id
    full_box(b"mvhd", 0, 0, &payload)
}

fn tkhd(movie_duration: u32, width: u32, height: u32) -> Vec<u8> {
    let mut payload = Vec::new();
    payload.extend_from_slice(&0u32.to_be_bytes()); // creation
    payload.extend_from_slice(&0u32.to_be_bytes()); // modification
    payload.extend_from_slice(&1u32.to_be_bytes()); // track_id
    payload.extend_from_slice(&0u32.to_be_bytes()); // reserved
    payload.extend_from_slice(&movie_duration.to_be_bytes());
    payload.extend_from_slice(&0u32.to_be_bytes());
    payload.extend_from_slice(&0u32.to_be_bytes());
    payload.extend_from_slice(&0u16.to_be_bytes()); // layer
    payload.extend_from_slice(&0u16.to_be_bytes()); // alternate_group
    payload.extend_from_slice(&0u16.to_be_bytes()); // volume (video)
    payload.extend_from_slice(&0u16.to_be_bytes()); // reserved
    payload.extend_from_slice(&identity_matrix());
    payload.extend_from_slice(&fixed_16_16(width));
    payload.extend_from_slice(&fixed_16_16(height));
    payload
}

fn mdhd(media_duration: u32) -> Vec<u8> {
    let mut payload = Vec::new();
    payload.extend_from_slice(&0u32.to_be_bytes()); // creation
    payload.extend_from_slice(&0u32.to_be_bytes()); // modification
    payload.extend_from_slice(&MEDIA_TIMESCALE.to_be_bytes());
    payload.extend_from_slice(&media_duration.to_be_bytes());
    payload.extend_from_slice(&0x55C4u16.to_be_bytes()); // "und"
    payload.extend_from_slice(&0u16.to_be_bytes()); // pre_defined
    full_box(b"mdhd", 0, 0, &payload)
}

fn hdlr() -> Vec<u8> {
    let mut payload = Vec::new();
    payload.extend_from_slice(&0u32.to_be_bytes()); // pre_defined
    payload.extend_from_slice(b"vide");
    for _ in 0..3 {
        payload.extend_from_slice(&0u32.to_be_bytes());
    }
    payload.extend_from_slice(b"VideoHandler\0");
    full_box(b"hdlr", 0, 0, &payload)
}

fn minf(
    samples: &[Sample],
    avcc: &[u8],
    width: u32,
    height: u32,
) -> Result<Vec<u8>, RecordingError> {
    let mut vmhd = Vec::new();
    vmhd.extend_from_slice(&0u16.to_be_bytes()); // graphics mode
    vmhd.extend_from_slice(&0u16.to_be_bytes());
    vmhd.extend_from_slice(&0u16.to_be_bytes());
    vmhd.extend_from_slice(&0u16.to_be_bytes());

    let mut dref = Vec::new();
    dref.extend_from_slice(&1u32.to_be_bytes());
    dref.extend(full_box(b"url ", 0, 1, &[])); // self-contained
    let dinf = box_bytes(b"dinf", &full_box(b"dref", 0, 0, &dref));

    let stbl = box_bytes(
        b"stbl",
        &stsd(avcc, width, height)?
            .into_iter()
            .chain(stts(samples))
            .chain(stss(samples))
            .chain(stsc())
            .chain(stsz(samples))
            .chain(stco(samples)?)
            .collect::<Vec<_>>(),
    );

    let mut media = full_box(b"vmhd", 0, 1, &vmhd);
    media.extend(dinf);
    media.extend(stbl);
    Ok(box_bytes(b"minf", &media))
}

fn stsd(avcc: &[u8], width: u32, height: u32) -> Result<Vec<u8>, RecordingError> {
    if width == 0 || height == 0 {
        return Err(RecordingError::Invalid(
            "video dimensions are unknown".to_owned(),
        ));
    }

    let mut entry = vec![0u8; 6]; // reserved
    entry.extend_from_slice(&1u16.to_be_bytes()); // data_reference_index
    entry.extend_from_slice(&0u16.to_be_bytes()); // pre_defined
    entry.extend_from_slice(&0u16.to_be_bytes()); // reserved
    for _ in 0..3 {
        entry.extend_from_slice(&0u32.to_be_bytes()); // pre_defined
    }
    entry.extend_from_slice(&(width as u16).to_be_bytes());
    entry.extend_from_slice(&(height as u16).to_be_bytes());
    entry.extend_from_slice(&0x0048_0000u32.to_be_bytes()); // horizontal resolution
    entry.extend_from_slice(&0x0048_0000u32.to_be_bytes()); // vertical resolution
    entry.extend_from_slice(&0u32.to_be_bytes()); // reserved
    entry.extend_from_slice(&1u16.to_be_bytes()); // frame count
    let mut compressor = vec![0u8; 32];
    compressor[0] = 0; // empty name
    entry.extend_from_slice(&compressor);
    entry.extend_from_slice(&0x0018u16.to_be_bytes()); // depth
    entry.extend_from_slice(&0xFFFFu16.to_be_bytes()); // pre_defined -1
    entry.extend(box_bytes(b"avcC", avcc));

    let mut payload = Vec::new();
    payload.extend_from_slice(&1u32.to_be_bytes()); // entry count
    payload.extend(box_bytes(b"avc1", &entry));
    Ok(full_box(b"stsd", 0, 0, &payload))
}

/// One `stts` entry per run of equal sample durations.
fn stts(samples: &[Sample]) -> Vec<u8> {
    let mut entries: Vec<(u32, u32)> = Vec::new();
    for sample in samples {
        let delta = sample.delta_us.min(u32::MAX as u64) as u32;
        if let Some(last) = entries.last_mut() {
            if last.1 == delta {
                last.0 += 1;
                continue;
            }
        }
        entries.push((1, delta));
    }

    let mut payload = Vec::new();
    payload.extend_from_slice(&(entries.len() as u32).to_be_bytes());
    for (count, delta) in entries {
        payload.extend_from_slice(&count.to_be_bytes());
        payload.extend_from_slice(&delta.to_be_bytes());
    }
    full_box(b"stts", 0, 0, &payload)
}

fn stss(samples: &[Sample]) -> Vec<u8> {
    let mut payload = Vec::new();
    let sync: Vec<u32> = samples
        .iter()
        .enumerate()
        .filter(|(_, sample)| sample.keyframe)
        .map(|(index, _)| (index + 1) as u32)
        .collect();
    payload.extend_from_slice(&(sync.len() as u32).to_be_bytes());
    for index in sync {
        payload.extend_from_slice(&index.to_be_bytes());
    }
    full_box(b"stss", 0, 0, &payload)
}

fn stsc() -> Vec<u8> {
    let mut payload = Vec::new();
    payload.extend_from_slice(&1u32.to_be_bytes()); // entry count
    payload.extend_from_slice(&1u32.to_be_bytes()); // first chunk
    payload.extend_from_slice(&1u32.to_be_bytes()); // samples per chunk
    payload.extend_from_slice(&1u32.to_be_bytes()); // description index
    full_box(b"stsc", 0, 0, &payload)
}

fn stsz(samples: &[Sample]) -> Vec<u8> {
    let mut payload = Vec::new();
    payload.extend_from_slice(&0u32.to_be_bytes()); // uniform size
    payload.extend_from_slice(&(samples.len() as u32).to_be_bytes());
    for sample in samples {
        payload.extend_from_slice(&sample.size.to_be_bytes());
    }
    full_box(b"stsz", 0, 0, &payload)
}

fn stco(samples: &[Sample]) -> Result<Vec<u8>, RecordingError> {
    let mut payload = Vec::new();
    payload.extend_from_slice(&(samples.len() as u32).to_be_bytes());
    for sample in samples {
        let offset = u32::try_from(sample.offset).map_err(|_| {
            RecordingError::Failed("MP4 exceeds the 32-bit chunk offset limit".to_owned())
        })?;
        payload.extend_from_slice(&offset.to_be_bytes());
    }
    Ok(full_box(b"stco", 0, 0, &payload))
}

fn box_bytes(kind: &[u8; 4], payload: &[u8]) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(8 + payload.len());
    bytes.extend_from_slice(&((payload.len() + 8) as u32).to_be_bytes());
    bytes.extend_from_slice(kind);
    bytes.extend_from_slice(payload);
    bytes
}

fn full_box(kind: &[u8; 4], version: u8, flags: u32, payload: &[u8]) -> Vec<u8> {
    let mut body = Vec::with_capacity(4 + payload.len());
    body.push(version);
    body.extend_from_slice(&flags.to_be_bytes()[1..]);
    body.extend_from_slice(payload);
    box_bytes(kind, &body)
}

fn identity_matrix() -> [u8; 36] {
    let mut matrix = [0u8; 36];
    for (offset, value) in [
        (0usize, 0x0001_0000u32),
        (16, 0x0001_0000),
        (32, 0x4000_0000),
    ] {
        matrix[offset..offset + 4].copy_from_slice(&value.to_be_bytes());
    }
    matrix
}

fn fixed_16_16(value: u32) -> [u8; 4] {
    (((value as u64) << 16) as u32).to_be_bytes()
}

fn write_all<W: Write>(writer: &mut W, bytes: &[u8]) -> Result<(), RecordingError> {
    writer.write_all(bytes).map_err(io_error)
}

fn io_error(error: io::Error) -> RecordingError {
    RecordingError::Failed(format!("MP4 I/O failed: {error}"))
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use super::*;

    const AVCC: &[u8] = &[1, 0x42, 0, 0, 0, 1, 0x67, 1, 0, 0, 0, 1, 0x68];

    fn writer() -> Mp4Writer<Cursor<Vec<u8>>> {
        Mp4Writer::new(Cursor::new(Vec::new()), Some(&(1080, 2400, AVCC))).expect("create writer")
    }

    fn kind(bytes: &[u8], offset: usize) -> &[u8] {
        &bytes[offset + 4..offset + 8]
    }

    fn children(bytes: &[u8], parent: usize, names: &[&[u8]]) -> Vec<(usize, usize)> {
        let mut found = Vec::new();
        let mut cursor = parent;
        while cursor + 8 <= bytes.len() {
            let size = u32::from_be_bytes(bytes[cursor..cursor + 4].try_into().unwrap()) as usize;
            assert!(size >= 8, "invalid box size");
            let name = kind(bytes, cursor);
            if names.contains(&name) {
                found.push((cursor, size));
            }
            cursor += size;
        }
        found
    }

    #[test]
    fn writes_the_top_level_boxes_and_patches_mdat_size() {
        let mut writer = writer();
        writer
            .write_frame(0, true, &[0, 0, 0, 2, 0x65])
            .expect("frame");
        writer
            .write_frame(33_000, false, &[0, 0, 0, 2, 0x41])
            .expect("frame");
        let stats = writer.finish().expect("finish");

        assert_eq!(stats.frames, 2);
        assert_eq!(stats.duration_us, 66_000, "both samples last one interval");

        let bytes = writer.inner.into_inner();
        let boxes = children(&bytes, 0, &[b"ftyp", b"mdat", b"moov"]);
        assert_eq!(boxes.len(), 3);
        assert_eq!(kind(&bytes, boxes[0].0), b"ftyp");
        assert_eq!(kind(&bytes, boxes[1].0), b"mdat");
        assert_eq!(kind(&bytes, boxes[2].0), b"moov");
        assert_eq!(boxes[1].1, 8 + 5 + 5, "mdat size must be patched");
        let mdat = boxes[1].0;
        let prefix = u32::from_be_bytes(bytes[mdat..mdat + 4].try_into().unwrap()) as usize;
        assert_eq!(prefix, boxes[1].1);
    }

    #[test]
    fn moov_contains_the_expected_sample_tables() {
        let mut writer = writer();
        writer.write_frame(0, true, &[1, 2, 3]).expect("frame");
        writer.write_frame(16_000, false, &[4, 5]).expect("frame");
        writer.write_frame(32_000, true, &[6]).expect("frame");
        writer.finish().expect("finish");

        let bytes = writer.inner.into_inner();
        let moov = children(&bytes, 0, &[b"moov"])[0].0;
        assert_eq!(children(&bytes, moov + 8, &[b"mvhd", b"trak"]).len(), 2);
        let trak = children(&bytes, moov + 8, &[b"trak"])[0].0;
        let mdia = children(&bytes, trak + 8, &[b"mdia"])[0].0;
        let minf = children(&bytes, mdia + 8, &[b"minf"])[0].0;
        let stbl = children(&bytes, minf + 8, &[b"stbl"])[0].0;
        let tables: Vec<&[u8]> = children(
            &bytes,
            stbl + 8,
            &[b"stsd", b"stts", b"stss", b"stsc", b"stsz", b"stco"],
        )
        .iter()
        .map(|(offset, _)| kind(&bytes, *offset))
        .collect();
        assert_eq!(
            tables,
            vec![b"stsd", b"stts", b"stss", b"stsc", b"stsz", b"stco"]
        );
    }

    #[test]
    fn rejects_frames_without_a_configuration() {
        let mut writer = Mp4Writer::new(Cursor::new(Vec::new()), None).expect("create writer");
        assert!(matches!(
            writer.write_frame(0, true, &[1]),
            Err(RecordingError::Invalid(_))
        ));
    }

    #[test]
    fn rejects_finishing_without_frames() {
        let mut writer = writer();
        assert!(matches!(writer.finish(), Err(RecordingError::Invalid(_))));
    }
}
