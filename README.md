# Elevator

![Logo](elevator.png)

This is a CLI application for validating and correcting the level syntax element in the sequence header(s) of AV1 streams.

Encoders tend to do a poor job of setting the level accurately, because headers are usually written before the rest of the stream.
Without enforcing constraints to keep encoded streams below a given level, estimating the correct one in advance is difficult to impossible.

Elevator parses a fully-encoded stream, calculates all the necessary parameters and determines the minimum acceptable level that will allow a spec-conformant decoder to decode it. It can then output this level to the command line, or patch it, either in place or to a new file.

## Restrictions
- Only IVF file input is supported
- Only one operating point is supported
- Some parameters are parsed from the first sequence header only, and are assumed to be consistent across sequences
- Some uncommon AV1 features, like scalability and super resolution, are untested and may produce incorrect output

## Usage
```
    elevator [FLAGS] [OPTIONS] <INPUT_FILE>

FLAGS:
    -h, --help       Prints help information
        --inplace    Patch file in place
    -V, --version    Prints version information
    -v, --verbose    Display verbose output, which may be helpful for debugging

OPTIONS:
    -f, --forcedlevel <FORCED_LEVEL>    Force a level instead of calculating it [possible values: 0, 1, 4, 5, 8, 9, 12,
                                        13, 14, 15, 16, 17, 18, 19, 31]
    -o, --output <OUTPUT_FILE>          Output filename

ARGS:
    <INPUT_FILE>    Input filename
```
