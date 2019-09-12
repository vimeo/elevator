extern crate av1parser;
extern crate clap;

mod ivf;
mod obu;

use av1parser as av1p;
use clap::{App, Arg};
use std::fs::File;
use std::io::{BufReader, Result, Seek, SeekFrom};

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
                .required(false)
                .index(2),
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

    // Open the specified input file using a buffered reader.
    let input_fname = matches.value_of("input").unwrap();
    let input_file = File::open(input_fname).expect("could not open the specified input file");
    let mut reader = BufReader::new(input_file);

    let fmt = av1p::probe_fileformat(&mut reader).expect("could not probe the input file format");
    reader.seek(SeekFrom::Start(0))?;

    let mut seq = av1p::av1::Sequence::new();
    let mut tile_info = av1p::obu::TileInfo::default();

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

                    if obu.obu_type == av1p::obu::OBU_FRAME_HEADER {
                        if seq.sh.is_none() {
                            panic!("frame header found before sequence header");
                        }

                        if let Some(fh) = av1p::obu::parse_frame_header(
                            &mut reader,
                            seq.sh.as_ref().unwrap(),
                            &mut seq.rfman,
                        ) {
                            tile_info = fh.tile_info;

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
                    } else {
                        obu::process_obu(&mut reader, &mut seq, &obu);
                    }

                    reader.seek(SeekFrom::Start(pos + obu.obu_size as u64))?;
                }

                reader.seek(SeekFrom::Start(pos + frame.size as u64))?;
            }

            // compute the level
            let sh = seq.sh.unwrap();
            println!(
                "tile config: {}x{}",
                tile_info.tile_cols, tile_info.tile_rows
            );
        }
        _ => {
            unimplemented!();
        }
    };

    Ok(())
}
