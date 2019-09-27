use av1parser::*;
use std::io;

// Adapted from av1parser. TODO: clean up/refactor/rewrite
pub fn process_obu<R: io::Read>(reader: &mut R, seq: &mut av1::Sequence, obu: &obu::Obu) {
    let reader = &mut io::Read::take(reader, u64::from(obu.obu_size));
    match obu.obu_type {
        obu::OBU_SEQUENCE_HEADER => {
            if let Some(sh) = obu::parse_sequence_header(reader) {
                seq.sh = Some(sh);
            }
        }
        obu::OBU_FRAME_HEADER | obu::OBU_FRAME => {
            if seq.sh.is_none() {
                return;
            }
            if let Some(fh) =
                obu::parse_frame_header(reader, seq.sh.as_ref().unwrap(), &mut seq.rfman)
            {
                // decode_frame_wrapup(): Decode frame wrapup process
                if fh.show_frame || fh.show_existing_frame {
                    seq.rfman.output_process(&fh);
                }
                if obu.obu_type == obu::OBU_FRAME {
                    seq.rfman.update_process(&fh);
                }
            }
        }
        _ => {}
    }
}
