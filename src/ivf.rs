use av1parser;
use std::io;

// Adapted from av1parser. TODO: clean up/refactor/rewrite
pub fn parse_ivf_header<R: io::Read + io::Seek>(
    mut reader: R,
    fname: &str,
) -> io::Result<av1parser::ivf::IvfHeader> {
    let mut ivf_header = [0; av1parser::ivf::IVF_HEADER_SIZE];
    reader.read_exact(&mut ivf_header)?;

    match av1parser::ivf::parse_ivf_header(&ivf_header) {
        Ok(header) => {
            if header.codec != av1parser::FCC_AV01 {
                panic!("{}: unsupport codec", fname);
            }

            Ok(header)
        }
        Err(msg) => {
            panic!("{}: {}", fname, msg);
        }
    }
}
