extern crate av1parser;
extern crate clap;

mod ivf;
mod obu;

use av1parser as av1p;
use clap::{App, Arg};
use std::fs::{File, OpenOptions};
use std::io::{BufReader, BufWriter, Read, Result, Seek, SeekFrom, Write};

fn main() -> Result<()> {
    /// Shortcut for fetching a Cargo environment variable.
    macro_rules! cargo_env {
        ($name: expr) => {
            env!(concat!("CARGO_PKG_", $name))
        };
    }

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
                .help("Output filename")
                .required(false),
        )
        .arg(
            Arg::with_name("inplace")
                .long("inplace")
                .help("Patch file in-place")
                .required(false),
        )
        .arg(
            Arg::with_name("forcedlevel")
                .short("l")
                .long("forcedlevel")
                .value_name("FORCED_LEVEL")
                .help("Force a level instead of calculating it")
                .required(false),
        )
        .get_matches();

    if matches.is_present("output") && matches.is_present("inplace") {
        panic!("cannot specify an output file and in-place at the same time");
    }

    let inplace = matches.is_present("inplace");

    if !matches.is_present("forcedlevel") {
        unimplemented!("automatic level computation is not yet supported");
    }

    let forced_level = matches.value_of("forcedlevel");
    if forced_level.is_some() {
        forced_level.unwrap().parse::<u8>().unwrap(); // check that the value is valid before processing
    }

    // Open the specified input file using a buffered reader.
    let input_fname = matches.value_of("input").unwrap();
    let output_fname = matches.value_of("output").unwrap_or(input_fname);

    let has_output_file = matches.is_present("output") || matches.is_present("inplace");

    let input_file = OpenOptions::new()
        .read(true)
        .write(inplace)
        .open(input_fname)
        .expect("could not open the specified input file");
    let output_file: File;

    let mut reader = BufReader::new(input_file);
    let mut writer: BufWriter<File>;

    let fmt = av1p::probe_fileformat(&mut reader).expect("could not probe the input file format");
    reader.seek(SeekFrom::Start(0))?;

    let mut seq = av1p::av1::Sequence::new();
    let mut seq_pos = 0;
    let mut seq_sz = 0;

    match fmt {
        av1p::FileFormat::IVF => {
            // Adapted from av1parser
            ivf::parse_ivf_header(&mut reader, input_fname)?;

            'ivf: while let Ok(frame) = av1p::ivf::parse_ivf_frame(&mut reader) {
                let mut sz = frame.size;
                let pos = reader.seek(SeekFrom::Current(0))?;
                while sz > 0 {
                    let obu = av1p::obu::parse_obu_header(&mut reader, sz)?;

                    sz -= obu.header_len + obu.obu_size;
                    let pos = reader.seek(SeekFrom::Current(0))?;

                    match obu.obu_type {
                        av1p::obu::OBU_FRAME_HEADER => {
                            if seq.sh.is_none() {
                                panic!("frame header found before sequence header");
                            }

                            if let Some(fh) = av1p::obu::parse_frame_header(
                                &mut reader,
                                seq.sh.as_ref().unwrap(),
                                &mut seq.rfman,
                            ) {
                                if fh.show_frame || fh.show_existing_frame {
                                    seq.rfman.output_process(&fh);
                                }
                                if obu.obu_type == av1p::obu::OBU_FRAME {
                                    seq.rfman.update_process(&fh);
                                }
                            }

                            // We currently assume the tile configuration is constant,
                            // so only one frame header needs to be processed.
                            break 'ivf;
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

                    reader.seek(SeekFrom::Start(pos + obu.obu_size as u64))?;
                }

                reader.seek(SeekFrom::Start(pos + frame.size as u64))?;
            }

            let sh = seq.sh.unwrap(); // sequence header

            // Determine the output level.
            let level: u8 = if let Some(level) = forced_level {
                level.parse().unwrap()
            } else {
                unimplemented!("level computation not yet supported")
            };

            let old_level = sh.op[0].seq_level_idx;

            if level > 7 && old_level <= 7 {
                unimplemented!(
                    "patching a tier in not yet supported"
                );
            }

            // Replace the level, if the output is to a file.
            if has_output_file {
                // Compute the location (offset) of the first operating point's level.
                // TODO: properly offset timing and decoder model info and any other missing data that is not decoded by av1parser
                if sh.operating_points_cnt > 1 {
                    unimplemented!("multiple operating points are not yet supported");
                }

                // Copy the file contents from input to output if needed.
                if !inplace {
                    std::fs::copy(input_fname, output_fname)?;
                }

                // Locate the first level byte by simply counting the bits that come before it.
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
                    (((level as u32) << 11 >> lv_bit_offset_in_byte) as u16).to_be_bytes();
                // Generate a two-byte mask to filter out the non-level bits.
                let level_bit_mask =
                    (((0b0001_1111_u32) << 11 >> lv_bit_offset_in_byte) as u16).to_be_bytes();
                // Generate a single bit mask to identify the tier bit, which immediately follows the level bits.
                let tier_bit_mask =
                    (((0b0000_0001_u32) << 11 >> lv_bit_offset_in_byte) as u16 >> 1).to_be_bytes();
                let post_tier_bit_mask =
                    (((0b1111_1111_1111_1111) << 3 >> lv_bit_offset_in_byte >> 8 >> 1) as u16)
                        .to_be_bytes();
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

                let mut byte_buf = [0_u8; 2];
                reader
                    .read(&mut byte_buf)
                    .expect("could not read the level byte(s)");

                // Ensure that the bytes read from the input file correspond to the level parsed earlier.
                assert_eq!(
                    old_level,
                    ((u16::from_be_bytes(byte_buf) as u32) >> 11 << lv_bit_offset_in_byte) as u8,
                    "level at the location seeked to patch does not match the parsed value"
                );

                print!(
                    "input/output bytes: {:#010b}, {:#010b} / ",
                    byte_buf[0], byte_buf[1]
                );

                // Modify the input bytes such that the level bits match the target level.
                byte_buf[0] = byte_buf[0] & !level_bit_mask[0] | level_aligned[0];
                byte_buf[1] = byte_buf[1] & !level_bit_mask[1] | level_aligned[1];

                let tier_adjusted_bits: [u8; 2];
                let mut next_input_byte = [0_u8; 1]; // when removing a tier bit (reader runs ahead)
                let mut _carry_bit = 0_u8; // used when adding a tier bit (reader runs behind)

                if old_level > 7 && level <= 7 {
                    // The tier bit must be removed.
                    // In that case, ensure that the tier bit is 0 (Main tier).
                    if byte_buf[0] & tier_bit_mask[0] > 0 || byte_buf[1] & tier_bit_mask[1] > 0 {
                        panic!("cannot reduce level below 4.0 when High tier is specified");
                    }

                    // Read one byte ahead, to shift the second byte in the current two-byte sequence.
                    reader
                        .read(&mut next_input_byte)
                        .expect("could not read the post-tier byte");

                    tier_adjusted_bits = [
                        (byte_buf[0] << 1) | (byte_buf[1] >> 7) & post_tier_bit_mask[0],
                        (byte_buf[1] << 1 | (next_input_byte[0] >> 7) & post_tier_bit_mask[1]),
                    ];
                } else if old_level <= 7 && level > 7 {
                    // The tier bit must be added.
                    tier_adjusted_bits = [
                        (byte_buf[0] >> 1) & !tier_bit_mask[0],
                        (byte_buf[1] >> 1) & !tier_bit_mask[1] | byte_buf[0] << 7,
                    ];

                    // The last bit is shifted out of the two-byte range, and must be
                    // stored to realign the rest of the bitstream. (TODO)
                    _carry_bit = byte_buf[1] << 7;
                } else {
                    // No adjustment is needed.
                    tier_adjusted_bits = byte_buf;
                }

                byte_buf[0] = level_aligned[0] | (tier_adjusted_bits[0] & (tier_bit_mask[0] | post_tier_bit_mask[0]));
                byte_buf[1] = level_aligned[1] | (tier_adjusted_bits[1] & (tier_bit_mask[1] | post_tier_bit_mask[1]));

                println!("{:#010b}, {:#010b}", byte_buf[0], byte_buf[1]);

                writer
                    .write_all(&byte_buf)
                    .expect("could not write the level byte(s)");

                // Realign the rest of the sequence header OBU if needed (i.e. if a tier bit is added/removed).
                let mut pos_in_seq; // writer's position within the sequence header

                if old_level > 7 && level <= 7 {
                    // Due to the earlier shifting, the reader is always one byte ahead.
                    pos_in_seq = lv_bit_offset_in_seq / 8 + 2;

                    while pos_in_seq < seq_sz.into() {
                        let prev_input_byte = next_input_byte;

                        reader
                            .read(&mut next_input_byte)
                            .expect("could not read sequence header OBU byte");

                        let next_output_byte =
                            (prev_input_byte[0] << 1) | (next_input_byte[0] >> 7);

                        writer
                            .write_all(&[next_output_byte])
                            .expect("could not write sequence header OBU byte");

                        pos_in_seq += 1;
                    }
                } else if old_level <= 7 && level > 7 {
                    // Due to the earlier shifting, the writer is always one byte ahead.
                    //pos_in_seq = lv_byte_offset + 3;

                    // TODO

                    unimplemented!("patching a tier bit into the bitstream not yet supported");
                }

                writer.flush()?;
            }

            println!("level: {} -> {}", old_level, level);
        }
        _ => {
            unimplemented!();
        }
    };

    Ok(())
}
