extern crate av1parser;
extern crate clap;

mod ivf;
mod level;
mod obu;

use av1parser as av1p;
use clap::{App, Arg};
use level::*;
use std::collections::VecDeque;
use std::fmt;
use std::fmt::{Display, Formatter};
use std::fs::{File, OpenOptions};
use std::io;
use std::io::{BufReader, BufWriter, Read, Seek, SeekFrom, Write};

#[derive(PartialEq)]
enum Output<'a> {
    InPlace,
    File(&'a str),
    CommandLine,
}

/// Configuration parameters received via CLI
struct AppConfig<'a> {
    verbose: bool,
    input: &'a str,
    output: Output<'a>,
    forced_level: Option<Level>,
}

/// Container-level stream metadata
struct ContainerMetadata {
    /// Temporal resolution, such that `time_scale` units represent one second of real time
    /// Represented as a rational (numerator, denominator)
    time_scale: (u32, u32),
    /// Frame width and height in pixels
    resolution: (u16, u16),
}

impl ContainerMetadata {
    /// Provides the time base in floating point form
    fn time_scale(&self) -> f64 {
        f64::from(self.time_scale.0) / f64::from(self.time_scale.1)
    }
}

impl Display for ContainerMetadata {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        writeln!(
            f,
            "Time scale: {:.3} ({}/{})",
            self.time_scale(), self.time_scale.0, self.time_scale.1
        )?;
        writeln!(f, "Resolution: {}x{}", self.resolution.0, self.resolution.1)?;

        Ok(())
    }
}

/// Container-level frame metadata
struct ContainerFrameMetadata {
    /// Size of the frame in bytes
    size: u32,
    /// Display timestamp of the frame at the time scale of the stream
    display_timestamp: u64,
}

impl Display for ContainerFrameMetadata {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        writeln!(f, "Frame @ {}: {} bytes", self.display_timestamp, self.size)
    }
}

fn main() -> io::Result<()> {
    /// Shortcut for fetching a Cargo environment variable.
    macro_rules! cargo_env {
        ($name: expr) => {
            env!(concat!("CARGO_PKG_", $name))
        };
    }

    // Generate a list of valid levels to validate the `forcedlevel` argument.
    let level_strings = LEVELS
        .iter()
        .filter(|&l| l.is_valid())
        .map(|&l| l.0.to_string())
        .collect::<Vec<_>>();

    // Define the command line interface.
    let matches = App::new(cargo_env!("NAME"))
        .version(cargo_env!("VERSION"))
        .author(cargo_env!("AUTHORS"))
        .about(cargo_env!("DESCRIPTION"))
        .arg(
            Arg::with_name("input")
                .short("i")
                .long("input")
                .value_name("INPUT_FILE")
                .help("Input filename")
                .required(true)
                .index(1),
        )
        .arg(
            Arg::with_name("output")
                .short("o")
                .long("output")
                .value_name("OUTPUT_FILE")
                .help("Output filename"),
        )
        .arg(
            Arg::with_name("inplace")
                .long("inplace")
                .help("Patch file in place"),
        )
        .arg(
            Arg::with_name("forcedlevel")
                .short("f")
                .long("forcedlevel")
                .value_name("FORCED_LEVEL")
                .help("Force a level instead of calculating it")
                .possible_values(&level_strings.iter().map(|l| &**l).collect::<Vec<_>>()),
        )
        .arg(
            Arg::with_name("verbose")
                .short("v")
                .long("verbose")
                .help("Display verbose output, which may be helpful for debugging"),
        )
        .get_matches();

    // Parse command line input.
    if matches.is_present("output") && matches.is_present("inplace") {
        panic!("cannot specify an output file and in place at the same time");
    }

    let config = AppConfig {
        verbose: matches.is_present("verbose"),
        input: matches.value_of("input").unwrap(),
        output: if matches.is_present("output") {
            Output::File(matches.value_of("output").unwrap())
        } else if matches.is_present("inplace") {
            Output::InPlace
        } else {
            Output::CommandLine
        },
        forced_level: if matches.is_present("forcedlevel") {
            Some(
                LEVELS[matches
                    .value_of("forcedlevel")
                    .unwrap()
                    .parse::<usize>()
                    .unwrap()],
            )
        } else {
            None
        },
    };

    process_input(&config)?;

    Ok(())
}

// TODO: split this function into smaller parts
#[allow(clippy::cognitive_complexity)]
fn process_input(config: &AppConfig) -> io::Result<()> {
    // Open the specified input file using a buffered reader.
    let input_file = OpenOptions::new()
        .read(true)
        .write(config.output == Output::InPlace)
        .open(config.input)
        .expect("could not open the specified input file");
    let output_file: File;

    let mut reader = BufReader::new(input_file);
    let mut writer: BufWriter<File>;

    let fmt = av1p::probe_fileformat(&mut reader).expect("could not probe the input file format");
    reader.seek(SeekFrom::Start(0))?;

    let mut seq = av1p::av1::Sequence::new();
    let mut seq_pos = 0;
    let mut seq_sz = 0;

    let (mut max_tile_cols, mut max_tiles) = (0, 0); // the maximum tile parameters
    let mut max_display_rate = 0_f64; // max number of shown frames in a temporal unit (i.e. number of frame headers with show_frame or show_existing_frame)
    let mut max_decode_rate = 0_f64; // max number of decoded frames in a temporal unit (i.e. number of frame headers without show_existing_frame)
    let mut max_header_rate = 0_f64; // max number of frame and frame header (excluding show_existing_frame) OBUs in a temporal unit
    let mut min_cr_level_idx = 0; // minimum level index required to support the compressed ratio bound
    let mut max_mbps = 0_f64; // max bitrate in megabits per second
    let mut max_tile_list_bitrate = 0; // max bitrate for tile lists
    let mut max_tile_decode_rate = 0_f64; // max decode rate for tile lists

    let metadata = match fmt {
        av1p::FileFormat::IVF => {
            let header = ivf::parse_ivf_header(&mut reader, config.input)?;

            ContainerMetadata {
                // Note: the `framerate` field name (from av1parser) is inaccurate
                time_scale: (header.framerate, header.timescale),
                resolution: (header.width, header.height),
            }
        }
        _ => unimplemented!("non-IVF input not currently supported"),
    };

    let time_scale = metadata.time_scale();
    let picture_size = usize::from(metadata.resolution.0) * usize::from(metadata.resolution.1);

    if config.verbose {
        println!("Container metadata:");
        println!("{}", metadata);
    }

    // TODO: do not parse the whole stream if setting a level manually
    let mut show_count = 0; // shown frame count for the current temporal unit
    let mut frame_count = 0; // decoded frame count for the current temporal unit
    let mut header_count = 0; // header count for the current temporal unit
    let mut last_tu_time = 0; // timestamp for the first frame of the last temporal unit
    let mut cur_tu_time = 0; // timestamp for the first frame of the current temporal unit
    let mut frame_size = 0_i64; // total compressed size for the current frame (includes frame, frame header, metadata, and tile group OBUs)
    let mut tu_size = 0; // total size of frames in the current temporal unit
    let mut tu_sizes = VecDeque::<u32>::new(); // one-second buffer for bitrate calculation per temporal unit
    let mut tu_times = VecDeque::<u64>::new(); // one-second buffer for time scale units taken per temporal unit
    let mut header_counts = VecDeque::<u32>::new(); // one-second buffer for number of headers per temporal unit
    let mut seen_frame_header = false; // refreshed with each temporal unit
    let mut min_compressed_ratio = std::f64::MAX; // min compression ratio for a single frame
    let mut tile_info = av1p::obu::TileInfo::default(); // last seen tile information

    let mut total_show_count = 0; // total number of displayed frames

    fn get_container_frame<R: io::Read>(
        reader: &mut R,
        fmt: &av1p::FileFormat,
    ) -> Option<ContainerFrameMetadata> {
        match fmt {
            av1p::FileFormat::IVF => {
                if let Ok(frame) = av1p::ivf::parse_ivf_frame(reader) {
                    ContainerFrameMetadata {
                        size: frame.size,
                        display_timestamp: frame.pts,
                    }
                    .into()
                } else {
                    None
                }
            }
            _ => {
                unreachable!();
            }
        }
    }

    // Read one frame from the container at a time.
    while let Some(frame) = get_container_frame(&mut reader, &fmt) {
        let mut sz = frame.size;
        let pts = frame.display_timestamp;

        let pos = reader.seek(SeekFrom::Current(0))?;

        // Read all AV1 OBUs in the container frame.
        while sz > 0 {
            let obu = av1p::obu::parse_obu_header(&mut reader, sz)?;

            sz -= obu.header_len + obu.obu_size;
            let pos = reader.seek(SeekFrom::Current(0))?;

            match obu.obu_type {
                av1p::obu::OBU_TEMPORAL_DELIMITER => {
                    if pts == cur_tu_time {
                        // duplicate temporal delimiter?
                        continue;
                    }

                    let delta_time = (pts - cur_tu_time) as f64 / time_scale;

                    let display_rate = f64::from(show_count) / delta_time;
                    max_display_rate = max_display_rate.max(display_rate);
                    max_decode_rate = max_decode_rate.max(f64::from(frame_count) / delta_time);
                    //max_header_rate = max_header_rate.max(header_count as f64 / delta_time);

                    // Calculate bitrate and header rate, windowed over one second (sampled every frame).
                    // We assume that header rate is computed over one-second windows.
                    // This is not clear in the specification, but seems implied.
                    header_counts.push_back(header_count);
                    tu_sizes.push_back(tu_size);
                    tu_times.push_back(pts - cur_tu_time);

                    let mut tu_times_sum = tu_times.iter().sum::<u64>() as f64;

                    if tu_times_sum >= time_scale.round() {
                        while tu_times_sum > time_scale.round() {
                            header_counts.pop_front();
                            tu_sizes.pop_front();
                            tu_times.pop_front();

                            tu_times_sum = tu_times.iter().sum::<u64>() as f64
                        }

                        let factor = time_scale / tu_times_sum; // adjustment to measure rates per second

                        let header_rate = f64::from(header_counts.iter().sum::<u32>()) * factor;
                        max_header_rate = max_header_rate.max(header_rate);

                        let mbps = f64::from(tu_sizes.iter().sum::<u32>())
                            * factor
                            * 8.0
                            / 1_000_000.0;
                        max_mbps = max_mbps.max(mbps);
                    }

                    if let Some(sh) = seq.sh {
                        let tier = if sh.op[0].seq_tier == 0 {
                            Tier::Main
                        } else {
                            Tier::High
                        };
                        let min_pic_compressed_ratio =
                            calculate_min_pic_compress_ratio(tier, display_rate);

                        for (level_idx, compressed_ratio) in
                            min_pic_compressed_ratio.iter().enumerate()
                        {
                            if min_compressed_ratio >= *compressed_ratio {
                                min_cr_level_idx = min_cr_level_idx.max(level_idx);
                                break;
                            }
                        }
                    }

                    total_show_count += show_count;

                    show_count = 0;
                    frame_count = 0;
                    header_count = 0;
                    tu_size = 0;
                    min_compressed_ratio = std::f64::MAX;
                    seen_frame_header = false;

                    obu::process_obu(&mut reader, &mut seq, &obu);
                }
                av1p::obu::OBU_FRAME_HEADER | av1p::obu::OBU_FRAME => {
                    if let Some(sh) = seq.sh {
                        if obu.obu_type == av1p::obu::OBU_FRAME_HEADER {
                            if frame_size > 0 {
                                let profile_factor = match sh.seq_profile {
                                    0 => 15,
                                    1 => 30,
                                    _ => 36,
                                };
                                let uncompressed_size = (picture_size * profile_factor) >> 3; // this assumes a fixed picture size}
                                min_compressed_ratio = min_compressed_ratio
                                    .min(uncompressed_size as f64 / frame_size as f64);
                            }

                            frame_size = i64::from(obu.obu_size) - 128; // this assumes one frame header per frame, coming before other OBUs for this frame
                            tu_size += obu.obu_size;
                        } else {
                            frame_size += i64::from(obu.obu_size);
                            tu_size += obu.obu_size;
                        }

                        if let Some(fh) = av1p::obu::parse_frame_header(
                            &mut reader,
                            seq.sh.as_ref().unwrap(),
                            &mut seq.rfman,
                        ) {
                            if !seen_frame_header {
                                last_tu_time = cur_tu_time;
                                cur_tu_time = pts;
                            }
                            seen_frame_header = true;

                            if fh.show_frame || fh.show_existing_frame {
                                show_count += 1;

                                seq.rfman.output_process(&fh);
                            }

                            if !fh.show_existing_frame {
                                header_count += 1; // TODO: detect and do not count duplicate frame headers
                                frame_count += 1;
                                seq.rfman.update_process(&fh);
                            }

                            tile_info = fh.tile_info;
                            max_tile_cols = max_tile_cols.max(fh.tile_info.tile_cols);
                            max_tiles =
                                max_tiles.max(fh.tile_info.tile_cols * fh.tile_info.tile_rows);
                        }
                    } else {
                        panic!("frame header found before sequence header");
                    }
                }
                av1p::obu::OBU_METADATA | av1p::obu::OBU_TILE_GROUP => {
                    frame_size += i64::from(obu.obu_size);
                    tu_size += obu.obu_size;
                }
                av1p::obu::OBU_TILE_LIST => {
                    if let Some(tile_list) = av1p::obu::parse_tile_list(&mut reader) {
                        let mut bytes_per_tile_list = 0;

                        for entry in tile_list.tile_list_entries {
                            bytes_per_tile_list += entry.tile_data_size_minus_1 + 1;
                        }

                        max_tile_list_bitrate =
                            max_tile_list_bitrate.max(bytes_per_tile_list * 8 * 180);
                        max_tile_decode_rate = max_tile_decode_rate.max(
                            f64::from(metadata.resolution.0) / f64::from(tile_info.tile_cols)
                                * f64::from(metadata.resolution.1)
                                / f64::from(tile_info.tile_rows)
                                * f64::from(tile_list.tile_count_minus_1 + 1)
                                * 180.0,
                        );
                    }
                }
                av1p::obu::OBU_SEQUENCE_HEADER => {
                    // Track the start location and size of the sequence header OBU for patching.
                    seq_pos = pos;
                    obu::process_obu(&mut reader, &mut seq, &obu);
                    seq_sz = obu.obu_size;
                }
                _ => {
                    obu::process_obu(&mut reader, &mut seq, &obu);
                }
            }

            reader.seek(SeekFrom::Start(pos + u64::from(obu.obu_size)))?;
        }

        reader.seek(SeekFrom::Start(pos + u64::from(frame.size)))?;
    }

    // Do the final updates for header/display/show rates.

    // Single frame clips don't move forward in time, so set a minimum delta of the framerate's inverse.
    let delta_time = ((cur_tu_time - last_tu_time) as f64 / time_scale).max(1.0 / time_scale * cur_tu_time as f64);
    let display_rate = f64::from(show_count) / delta_time;
    max_display_rate = max_display_rate.max(display_rate);
    max_decode_rate = max_decode_rate
        .max(f64::from(frame_count) / delta_time)
        // Tile decode rate is restricted to the level's maximum decode rate halved, so double the input to achieve that effect.
        .max(max_tile_decode_rate * 2.0);

    header_counts.push_back(header_count);
    tu_sizes.push_back(tu_size);
    tu_times.push_back(cur_tu_time - last_tu_time);

    let mut tu_times_sum = tu_times.iter().sum::<u64>() as f64;

    // We do not want to interpolate for short clips, since their effective rate per second is the same as their total rate.
    // However, for clips that fill the one-second buffers, interpolation should occur for the last frame as well.
    let factor = if tu_times_sum >= time_scale.round() { time_scale / tu_times_sum } else { 1.0 };

    while tu_times_sum > time_scale.round() {
        header_counts.pop_front();
        tu_sizes.pop_front();
        tu_times.pop_front();

        tu_times_sum = tu_times.iter().sum::<u64>() as f64
    }

    let header_rate = f64::from(header_counts.iter().sum::<u32>()) * factor;
    max_header_rate = max_header_rate.max(header_rate);

    let mbps = f64::from(tu_sizes.iter().sum::<u32>()) * factor * 8.0 / 1_000_000.0;
    max_mbps = max_mbps.max(mbps);

    let sh = seq.sh.unwrap(); // sequence header
    let tier = if sh.op[0].seq_tier == 0 {
        Tier::Main
    } else {
        Tier::High
    };
    let min_pic_compressed_ratio = calculate_min_pic_compress_ratio(tier, display_rate);

    for (level_idx, compressed_ratio) in min_pic_compressed_ratio.iter().enumerate() {
        if min_compressed_ratio >= *compressed_ratio {
            min_cr_level_idx = min_cr_level_idx.max(level_idx);
            break;
        }
    }

    total_show_count += show_count;

    if sh.operating_points_cnt > 1 {
        unimplemented!("streams with multiple operating points not yet supported");
    }

    if config.verbose {
        println!("Number of displayed frames: {}", total_show_count);

        println!(
            "Maximum header, display, and decode rates in a single temporal unit: {:.3}, {:.3}, {:.3}",
            max_header_rate, max_display_rate, max_decode_rate
        );

        println!(
            "Minimum level required to satisfy compressed ratio constraint: {}",
            LEVELS[min_cr_level_idx]
        );

        println!("Maximum bitrate: {:.3} Mbps", max_mbps);

        println!(
            "Maximum number of tiles and tile columns found: {}, {}",
            max_tiles, max_tile_cols
        );
    }

    // Determine the output level.
    let level: Level = if config.forced_level.is_some() {
        config.forced_level.unwrap()
    } else {
        // Generate a SequenceContext using the parsed data.
        let seq_ctx = SequenceContext {
            tier: if sh.op[0].seq_tier == 0 {
                Tier::Main
            } else {
                Tier::High
            },
            pic_size: (sh.max_frame_width as u16, sh.max_frame_height as u16), // (width, height)
            display_rate: (max_display_rate * picture_size as f64).ceil() as u64,
            decode_rate: (max_decode_rate * picture_size as f64).ceil() as u64,
            header_rate: max_header_rate.ceil() as u16,
            mbps: max_mbps,
            tiles: max_tiles as u8,
            tile_cols: max_tile_cols as u8,
        };

        if config.verbose {
            println!();
            println!("Sequence context:");
            println!("{}", seq_ctx);
        }
        LEVELS[usize::from(calculate_level(&seq_ctx).0).max(min_cr_level_idx)]
    };

    let old_level = &LEVELS[usize::from(sh.op[0].seq_level_idx)];

    // Replace the level, if the output is to a file.
    if config.output != Output::CommandLine {
        // Copy the file contents from input to output if needed.
        let output_fname = match config.output {
            Output::InPlace => config.input,
            Output::File(fname) => fname,
            _ => unreachable!(),
        };

        if config.output == Output::File(output_fname) {
            std::fs::copy(config.input, output_fname)?;
        }

        // Locate the first level byte by simply counting the bits that come before it.
        // This is only valid for single operating point sequences.
        // TODO: properly offset timing and decoder model info and any other missing data that is not decoded by av1parser
        let lv_bit_offset_in_seq = if sh.reduced_still_picture_header {
            5
        } else {
            // When timing info is present, there may be more nested header data to skip,
            // but it is not currently handled by av1parser or coded by rav1e.
            24 + if sh.timing_info_present_flag {
                unimplemented!()
            } else {
                0
            }
        };

        output_file = OpenOptions::new()
            .write(true)
            .open(output_fname)
            .expect("could not open the specified output file");
        writer = BufWriter::new(output_file);

        // Both the reader and writer should point to the first byte which contains level bits.
        let lv_byte_offset = seq_pos + lv_bit_offset_in_seq / 8;
        reader.seek(SeekFrom::Start(lv_byte_offset))?;
        writer.seek(SeekFrom::Start(lv_byte_offset))?;

        // Determine the number of bits preceding the level in the byte.
        let lv_bit_offset_in_byte = lv_bit_offset_in_seq % 8;

        // Generate a bitstream-aligned two-byte sequence containing the level bits.
        let level_aligned =
            ((u32::from(level.0) << 11 >> lv_bit_offset_in_byte) as u16).to_be_bytes();
        // Generate a two-byte mask to filter out the non-level bits.
        let level_bit_mask =
            (((0b0001_1111_u32) << 11 >> lv_bit_offset_in_byte) as u16).to_be_bytes();
        // Generate a single bit mask to identify the tier bit, which immediately follows the level bits.
        let tier_bit_mask =
            (((0b0000_0001_u32) << 11 >> lv_bit_offset_in_byte) as u16 >> 1).to_be_bytes();
        let post_tier_bit_mask = (((0b1111_1111_1111_1111) << 3 >> lv_bit_offset_in_byte >> 8 >> 1)
            as u16)
            .to_be_bytes();

        if config.verbose {
            println!(
                "offset: {} | level bits: {:#010b}, {:#010b}",
                lv_bit_offset_in_byte, level_aligned[0], level_aligned[1]
            );

            println!(
                "level/tier/post-tier bit masks: {:#018b} / {:#018b} / {:#018b}",
                u16::from_be_bytes(level_bit_mask),
                u16::from_be_bytes(tier_bit_mask),
                u16::from_be_bytes(post_tier_bit_mask)
            );
        }

        let mut byte_buf = [0_u8; 2];
        reader
            .read_exact(&mut byte_buf)
            .expect("could not read the level byte(s)");

        // Ensure that the bytes read from the input file correspond to the level parsed earlier.
        assert_eq!(
            old_level.0,
            (u32::from(u16::from_be_bytes(byte_buf)) >> 11 << lv_bit_offset_in_byte) as u8,
            "level at the location seeked to patch does not match the parsed value"
        );

        if config.verbose {
            print!(
                "input/output bytes: {:#010b}, {:#010b} / ",
                byte_buf[0], byte_buf[1]
            );
        }

        // Modify the input bytes such that the level bits match the target level.
        byte_buf[0] = byte_buf[0] & !level_bit_mask[0] | level_aligned[0];
        byte_buf[1] = byte_buf[1] & !level_bit_mask[1] | level_aligned[1];

        let tier_adjusted_bits: [u8; 2];
        let mut next_input_byte = [0_u8; 1]; // when removing a tier bit (reader runs ahead)
        let mut carry_bit = 0_u8; // used when adding a tier bit (reader runs behind)

        if old_level.0 > 7 && level.0 <= 7 {
            // The tier bit must be removed.
            // In that case, ensure that the tier bit is 0 (Main tier).
            if byte_buf[0] & tier_bit_mask[0] > 0 || byte_buf[1] & tier_bit_mask[1] > 0 {
                panic!("cannot reduce level below 4.0 when High tier is specified");
            }

            // Read one byte ahead, to shift the second byte in the current two-byte sequence.
            reader
                .read_exact(&mut next_input_byte)
                .expect("could not read the post-tier byte");

            tier_adjusted_bits = [
                (byte_buf[0] << 1) | (byte_buf[1] >> 7) & post_tier_bit_mask[0],
                (byte_buf[1] << 1 | (next_input_byte[0] >> 7) & post_tier_bit_mask[1]),
            ];
        } else if old_level.0 <= 7 && level.0 > 7 {
            // The tier bit must be added.
            tier_adjusted_bits = [
                (byte_buf[0] >> 1) & !tier_bit_mask[0],
                (byte_buf[1] >> 1) & !tier_bit_mask[1] | byte_buf[0] << 7,
            ];

            // The last bit is shifted out of the two-byte range, and must be
            // stored to realign the rest of the bitstream. (TODO)
            carry_bit = byte_buf[1] << 7;
        } else {
            // No adjustment is needed.
            tier_adjusted_bits = byte_buf;
        }

        byte_buf[0] =
            level_aligned[0] | (tier_adjusted_bits[0] & (tier_bit_mask[0] | post_tier_bit_mask[0]));
        byte_buf[1] =
            level_aligned[1] | (tier_adjusted_bits[1] & (tier_bit_mask[1] | post_tier_bit_mask[1]));

        if config.verbose {
            println!("{:#010b}, {:#010b}", byte_buf[0], byte_buf[1]);
        }

        writer
            .write_all(&byte_buf)
            .expect("could not write the level byte(s)");

        // Realign the rest of the sequence header OBU if needed (i.e. if a tier bit is added/removed).
        let mut pos_in_seq = lv_bit_offset_in_seq / 8 + 2; // writer's position within the sequence header
        let mut next_output_byte: u8;

        while pos_in_seq < seq_sz.into() {
            if old_level.0 > 7 && level.0 <= 7 {
                // Due to the earlier shifting, the reader is always one byte ahead.
                let prev_input_byte = next_input_byte;

                reader
                    .read_exact(&mut next_input_byte)
                    .expect("could not read sequence header OBU byte");

                next_output_byte = (prev_input_byte[0] << 1) | (next_input_byte[0] >> 7);
            } else if old_level.0 <= 7 && level.0 > 7 {
                reader
                    .read_exact(&mut next_input_byte)
                    .expect("could not read sequence header OBU byte");

                next_output_byte = next_input_byte[0] >> 1 | carry_bit;
                carry_bit = next_input_byte[0] << 7;
            } else {
                break;
            }

            writer
                .write_all(&[next_output_byte])
                .expect("could not write sequence header OBU byte");

            pos_in_seq += 1;
        }

        writer.flush()?;
    }

    println!("Level: {} -> {}", old_level, level);

    Ok(())
}
