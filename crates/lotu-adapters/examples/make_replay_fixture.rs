//! Builds a `.lotureplay` container from an Annex B H.264 clip.
//!
//! Offline fixture tool, **not** a production pipeline. The H.264 work
//! (Annex B → AVCC, `avcC`, IDR detection) is the production normalizer in
//! `lotu_adapters::media::h264`; this example only adds the fixture policy
//! (fixed 30 fps PTS and the M0 default resolution).
//!
//! ```text
//! cargo run -p lotu-adapters --example make_replay_fixture -- <in.h264> <out.lotureplay> [WxH]
//! ```
//!
//! The sibling `.json` metadata is maintained by hand next to the generated
//! container.

use std::error::Error;
use std::path::PathBuf;

use lotu_adapters::media::h264::convert_annex_b;
use lotu_adapters::replay::fixture::{encode_fixture, FixtureFrame, HELLO_JSON};

const FPS: u32 = 30;
const DEFAULT_SIZE: (u32, u32) = (1080, 2400);
const USAGE: &str = "usage: make_replay_fixture <in.h264> <out.lotureplay> [WxH]";

fn main() {
    if let Err(error) = run() {
        eprintln!("make_replay_fixture: {error}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), Box<dyn Error + Send + Sync>> {
    let mut args = std::env::args().skip(1);

    let input = args.next().ok_or(USAGE)?;
    let output = args.next().ok_or(USAGE)?;
    let (width, height) = match args.next() {
        Some(raw) => parse_size(&raw)?,
        None => DEFAULT_SIZE,
    };
    if args.next().is_some() {
        return Err(format!("unexpected extra arguments; {USAGE}").into());
    }

    let annex_b =
        std::fs::read(&input).map_err(|error| format!("cannot read input {}: {error}", input))?;
    let clip = convert_annex_b(&annex_b)?;

    let interval_us = 1_000_000 / i64::from(FPS);
    let mut frames = Vec::with_capacity(clip.samples.len());
    for (index, sample) in clip.samples.into_iter().enumerate() {
        let index = i64::try_from(index).map_err(|_| "frame index does not fit in i64")?;
        frames.push(FixtureFrame {
            pts_us: index * interval_us,
            keyframe: sample.keyframe,
            data: sample.data,
        });
    }

    let container = encode_fixture(HELLO_JSON, width, height, &clip.avcc, &frames)?;

    let output = PathBuf::from(output);
    std::fs::write(&output, &container)
        .map_err(|error| format!("cannot write {}: {error}", output.display()))?;

    println!(
        "wrote {} ({}x{}, {} frames, {} bytes)",
        output.display(),
        width,
        height,
        frames.len(),
        container.len()
    );

    Ok(())
}

fn parse_size(raw: &str) -> Result<(u32, u32), String> {
    let (width, height) = raw
        .split_once('x')
        .or_else(|| raw.split_once('X'))
        .ok_or_else(|| format!("invalid size {raw:?}; expected WxH"))?;

    let width = width
        .parse::<u32>()
        .map_err(|_| format!("invalid width in {raw:?}"))?;
    let height = height
        .parse::<u32>()
        .map_err(|_| format!("invalid height in {raw:?}"))?;

    if width == 0 || height == 0 {
        return Err(format!("invalid size {raw:?}; dimensions must be positive"));
    }

    Ok((width, height))
}
