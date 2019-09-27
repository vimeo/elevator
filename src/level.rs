use std::fmt::{Display, Formatter, Result};

#[derive(Debug, PartialEq)]
pub enum Tier {
    Main,
    High,
}

impl Default for Tier {
    fn default() -> Self {
        Tier::Main
    }
}

/// Describes the maximum parameters relevant to level restrictions
/// encountered in a sequence.
pub struct SequenceContext {
    pub tier: Tier,
    pub pic_size: (u16, u16), // (width, height)
    pub display_rate: u64,
    pub decode_rate: u64,
    pub header_rate: u16,
    pub mbps: f64,
    pub tiles: u8,
    pub tile_cols: u8,
}

impl Display for SequenceContext {
    fn fmt(&self, f: &mut Formatter<'_>) -> Result {
        writeln!(f, "Tier: {:?}", self.tier)?;
        writeln!(f, "Picture Size: {}x{}", self.pic_size.0, self.pic_size.1)?;
        writeln!(
            f,
            "Display/Decode/Header Rates: {}/{}/{}",
            self.display_rate, self.decode_rate, self.header_rate
        )?;
        writeln!(f, "Mbps: {:.3}", self.mbps)?;
        writeln!(f, "Tiles/Tile Columns: {}/{}", self.tiles, self.tile_cols)?;

        Ok(())
    }
}

#[derive(Copy, Clone)]
struct LevelLimits {
    max_pic_size: u32,
    max_h_size: u16,
    max_v_size: u16,
    max_display_rate: u64,
    max_decode_rate: u64,
    max_header_rate: u16,
    main_mbps: f64,
    high_mbps: f64,
    main_cr: u8,
    high_cr: u8,
    max_tiles: u8,
    max_tile_cols: u8,
}

#[derive(Copy, Clone)]
pub struct Level(pub u8, Option<LevelLimits>);

impl Level {
    pub fn is_valid(&self) -> bool {
        self.1.is_some()
    }
}

impl Display for Level {
    fn fmt(&self, f: &mut Formatter<'_>) -> Result {
        let index = self.0;

        if index == 31 {
            write!(f, "Maximum parameters")
        } else if index >= 24 {
            write!(f, "Reserved")
        } else {
            let x = 2 + (index >> 2);
            let y = index & 3;

            write!(f, "{}.{} ({})", x, y, self.0)
        }
    }
}

macro_rules! level {
    ($level: expr, $limits: expr) => {
        Level($level, Some($limits))
    };
    ($level: expr) => {
        Level($level, None)
    };
}

pub const LEVELS: [Level; 32] = [
    level!(
        0,
        LevelLimits {
            max_pic_size: 147_456,
            max_h_size: 2048,
            max_v_size: 1152,
            max_display_rate: 4_423_680,
            max_decode_rate: 5_529_600,
            max_header_rate: 150,
            main_mbps: 1.5,
            high_mbps: 0.0,
            main_cr: 2,
            high_cr: 0,
            max_tiles: 8,
            max_tile_cols: 4,
        }
    ),
    level!(
        1,
        LevelLimits {
            max_pic_size: 278_784,
            max_h_size: 2816,
            max_v_size: 1584,
            max_display_rate: 8_363_520,
            max_decode_rate: 10_454_400,
            max_header_rate: 150,
            main_mbps: 3.0,
            high_mbps: 0.0,
            main_cr: 2,
            high_cr: 0,
            max_tiles: 8,
            max_tile_cols: 4,
        }
    ),
    level!(2),
    level!(3),
    level!(
        4,
        LevelLimits {
            max_pic_size: 665_856,
            max_h_size: 4352,
            max_v_size: 2448,
            max_display_rate: 19_975_680,
            max_decode_rate: 24_969_600,
            max_header_rate: 150,
            main_mbps: 6.0,
            high_mbps: 0.0,
            main_cr: 2,
            high_cr: 0,
            max_tiles: 16,
            max_tile_cols: 6,
        }
    ),
    level!(
        5,
        LevelLimits {
            max_pic_size: 1_065_024,
            max_h_size: 5504,
            max_v_size: 3096,
            max_display_rate: 31_950_720,
            max_decode_rate: 39_938_400,
            max_header_rate: 150,
            main_mbps: 10.0,
            high_mbps: 0.0,
            main_cr: 2,
            high_cr: 0,
            max_tiles: 8,
            max_tile_cols: 4,
        }
    ),
    level!(6),
    level!(7),
    level!(
        8,
        LevelLimits {
            max_pic_size: 2_359_296,
            max_h_size: 6144,
            max_v_size: 3456,
            max_display_rate: 70_778_880,
            max_decode_rate: 77_856_768,
            max_header_rate: 300,
            main_mbps: 12.0,
            high_mbps: 30.0,
            main_cr: 4,
            high_cr: 4,
            max_tiles: 32,
            max_tile_cols: 8,
        }
    ),
    level!(
        9,
        LevelLimits {
            max_pic_size: 2_359_296,
            max_h_size: 6144,
            max_v_size: 3456,
            max_display_rate: 141_557_760,
            max_decode_rate: 155_713_536,
            max_header_rate: 300,
            main_mbps: 20.0,
            high_mbps: 50.0,
            main_cr: 4,
            high_cr: 4,
            max_tiles: 32,
            max_tile_cols: 8,
        }
    ),
    level!(10),
    level!(11),
    level!(
        12,
        LevelLimits {
            max_pic_size: 8_912_896,
            max_h_size: 8192,
            max_v_size: 4352,
            max_display_rate: 267_386_880,
            max_decode_rate: 273_715_200,
            max_header_rate: 300,
            main_mbps: 30.0,
            high_mbps: 100.0,
            main_cr: 6,
            high_cr: 4,
            max_tiles: 64,
            max_tile_cols: 8,
        }
    ),
    level!(
        13,
        LevelLimits {
            max_pic_size: 8_912_896,
            max_h_size: 8192,
            max_v_size: 4352,
            max_display_rate: 534_773_760,
            max_decode_rate: 547_430_400,
            max_header_rate: 300,
            main_mbps: 40.0,
            high_mbps: 160.0,
            main_cr: 8,
            high_cr: 4,
            max_tiles: 64,
            max_tile_cols: 8,
        }
    ),
    level!(
        14,
        LevelLimits {
            max_pic_size: 8_912_896,
            max_h_size: 8192,
            max_v_size: 4352,
            max_display_rate: 1_069_547_520,
            max_decode_rate: 1_094_860_800,
            max_header_rate: 300,
            main_mbps: 60.0,
            high_mbps: 240.0,
            main_cr: 8,
            high_cr: 4,
            max_tiles: 64,
            max_tile_cols: 8,
        }
    ),
    level!(
        15,
        LevelLimits {
            max_pic_size: 8_912_896,
            max_h_size: 8192,
            max_v_size: 4352,
            max_display_rate: 1_069_547_520,
            max_decode_rate: 1_176_502_272,
            max_header_rate: 300,
            main_mbps: 60.0,
            high_mbps: 240.0,
            main_cr: 8,
            high_cr: 4,
            max_tiles: 64,
            max_tile_cols: 8,
        }
    ),
    level!(
        16,
        LevelLimits {
            max_pic_size: 35_651_584,
            max_h_size: 16384,
            max_v_size: 8704,
            max_display_rate: 1_069_547_520,
            max_decode_rate: 1_176_502_272,
            max_header_rate: 300,
            main_mbps: 60.0,
            high_mbps: 240.0,
            main_cr: 8,
            high_cr: 4,
            max_tiles: 128,
            max_tile_cols: 16,
        }
    ),
    level!(
        17,
        LevelLimits {
            max_pic_size: 35_651_584,
            max_h_size: 16384,
            max_v_size: 8704,
            max_display_rate: 2_139_095_040,
            max_decode_rate: 2_189_721_600,
            max_header_rate: 300,
            main_mbps: 100.0,
            high_mbps: 480.0,
            main_cr: 8,
            high_cr: 4,
            max_tiles: 128,
            max_tile_cols: 16,
        }
    ),
    level!(
        18,
        LevelLimits {
            max_pic_size: 35_651_584,
            max_h_size: 16384,
            max_v_size: 8704,
            max_display_rate: 4_278_190_080,
            max_decode_rate: 4_379_443_200,
            max_header_rate: 300,
            main_mbps: 160.0,
            high_mbps: 800.0,
            main_cr: 8,
            high_cr: 4,
            max_tiles: 128,
            max_tile_cols: 16,
        }
    ),
    level!(
        19,
        LevelLimits {
            max_pic_size: 35_651_584,
            max_h_size: 16384,
            max_v_size: 8704,
            max_display_rate: 4_278_190_080,
            max_decode_rate: 4_706_009_088,
            max_header_rate: 300,
            main_mbps: 160.0,
            high_mbps: 800.0,
            main_cr: 8,
            high_cr: 4,
            max_tiles: 128,
            max_tile_cols: 16,
        }
    ),
    level!(20),
    level!(21),
    level!(22),
    level!(23),
    level!(24),
    level!(25),
    level!(26),
    level!(27),
    level!(28),
    level!(29),
    level!(30),
    level!(
        31,
        LevelLimits {
            max_pic_size: std::u32::MAX,
            max_h_size: std::u16::MAX,
            max_v_size: std::u16::MAX,
            max_display_rate: std::u64::MAX,
            max_decode_rate: std::u64::MAX,
            max_header_rate: std::u16::MAX,
            main_mbps: std::f64::MAX,
            high_mbps: std::f64::MAX,
            main_cr: std::u8::MAX,
            high_cr: std::u8::MAX,
            max_tiles: std::u8::MAX,
            max_tile_cols: std::u8::MAX,
        }
    ),
];

pub fn calculate_min_pic_compress_ratio(tier: Tier, display_rate: f64) -> [f64; 32] {
    let mut min_pic_compress_ratio = [0_f64; 32];

    for i in 0..32 {
        if let Some(limits) = LEVELS[i].1 {
            let speed_adjustment = display_rate / limits.max_display_rate as f64;
            let min_comp_basis = if tier == Tier::Main || i <= 7 {
                limits.main_cr
            } else {
                limits.high_cr
            };

            // assuming still_picture is equal to 0
            min_pic_compress_ratio[i] = 0.8_f64.max(f64::from(min_comp_basis) * speed_adjustment);
        }
    }

    min_pic_compress_ratio
}

pub fn calculate_level(context: &SequenceContext) -> Level {
    for level in LEVELS.iter() {
        if let Some(limits) = level.1 {
            // Only Main tier exists for low levels.
            let mbps_valid = if context.tier == Tier::Main || level.0 <= 7 {
                limits.main_mbps >= context.mbps
            } else {
                limits.high_mbps >= context.mbps
            };

            if limits.max_pic_size >= u32::from(context.pic_size.0) * u32::from(context.pic_size.1)
                && limits.max_h_size >= context.pic_size.0
                && limits.max_v_size >= context.pic_size.1
                && limits.max_display_rate >= context.display_rate
                && limits.max_decode_rate >= context.decode_rate
                && limits.max_header_rate >= context.header_rate
                && mbps_valid
                && limits.max_tiles >= context.tiles
                && limits.max_tile_cols >= context.tile_cols
            {
                return *level;
            }
        }
    }

    unreachable!("no suitable level found");
}
