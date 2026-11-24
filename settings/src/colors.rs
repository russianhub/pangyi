//! 候选窗预览用的颜色。
//!
//! 真源是 `ime/src/settings.rs` 的 `ACCENTS` 和 `ime/src/cand.rs` 的 `palette()`。
//! 这里抄一份，是为了不让设置程序依赖输入法包。改色必须两边一起改。

/// 候选窗词的字号基准（96 DPI 下的逻辑像素）。
pub const TEXT_PX: i32 = 16;
/// 俄语注释字号基准。
pub const ANNO_PX: i32 = 12;
/// 序号字号基准。
pub const INDEX_PX: i32 = 11;

/// 设置里的主题值怎么对应到窗口配色。
/// 数值故意和 Slint `ColorScheme` 错开：那边是 Unknown=0、Dark=1、Light=2。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SchemeKind {
    /// 跟随系统。
    Follow,
    Light,
    Dark,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rgb {
    pub r: u8,
    pub g: u8,
    pub b: u8,
}

impl Rgb {
    pub const fn new(r: u8, g: u8, b: u8) -> Self {
        Self { r, g, b }
    }

    pub fn hex(self) -> String {
        format!("#{:02X}{:02X}{:02X}", self.r, self.g, self.b)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Accent {
    pub fill: Rgb,
    pub text: Rgb,
    pub strong: Rgb,
    pub on_strong: Rgb,
}

/// `[配色 0–4][0 浅 / 1 深]`，与 `settings.rs` 的 `ACCENTS` 同一下标。
pub const ACCENTS: [[Accent; 2]; 5] = [
    [
        Accent {
            fill: Rgb::new(0xdd, 0xf2, 0xf6),
            text: Rgb::new(0x17, 0x60, 0x6c),
            strong: Rgb::new(0x2f, 0xb8, 0xcc),
            on_strong: Rgb::new(0xff, 0xff, 0xff),
        },
        Accent {
            fill: Rgb::new(0x1e, 0x3a, 0x44),
            text: Rgb::new(0xb7, 0xe6, 0xf0),
            strong: Rgb::new(0x40, 0xc8, 0xe0),
            on_strong: Rgb::new(0x06, 0x22, 0x2a),
        },
    ],
    [
        Accent {
            fill: Rgb::new(0xf8, 0xe4, 0xec),
            text: Rgb::new(0x82, 0x2e, 0x54),
            strong: Rgb::new(0xd4, 0x6a, 0x9c),
            on_strong: Rgb::new(0xff, 0xff, 0xff),
        },
        Accent {
            fill: Rgb::new(0x44, 0x22, 0x34),
            text: Rgb::new(0xf4, 0xc8, 0xda),
            strong: Rgb::new(0xe0, 0x7a, 0xb0),
            on_strong: Rgb::new(0x2a, 0x0c, 0x18),
        },
    ],
    [
        Accent {
            fill: Rgb::new(0xfa, 0xee, 0xe0),
            text: Rgb::new(0x8a, 0x4a, 0x1a),
            strong: Rgb::new(0xe0, 0x96, 0x40),
            on_strong: Rgb::new(0xff, 0xff, 0xff),
        },
        Accent {
            fill: Rgb::new(0x44, 0x30, 0x1e),
            text: Rgb::new(0xf4, 0xd6, 0xb0),
            strong: Rgb::new(0xe8, 0xa0, 0x4a),
            on_strong: Rgb::new(0x2a, 0x18, 0x06),
        },
    ],
    [
        Accent {
            fill: Rgb::new(0xee, 0xe6, 0xfa),
            text: Rgb::new(0x5a, 0x3a, 0x96),
            strong: Rgb::new(0x9a, 0x70, 0xd8),
            on_strong: Rgb::new(0xff, 0xff, 0xff),
        },
        Accent {
            fill: Rgb::new(0x32, 0x26, 0x4a),
            text: Rgb::new(0xd6, 0xc6, 0xf4),
            strong: Rgb::new(0xa8, 0x82, 0xe4),
            on_strong: Rgb::new(0x1a, 0x10, 0x2e),
        },
    ],
    [
        Accent {
            fill: Rgb::new(0xe4, 0xf4, 0xe8),
            text: Rgb::new(0x26, 0x68, 0x3c),
            strong: Rgb::new(0x3e, 0xb0, 0x60),
            on_strong: Rgb::new(0xff, 0xff, 0xff),
        },
        Accent {
            fill: Rgb::new(0x1e, 0x3c, 0x2a),
            text: Rgb::new(0xc8, 0xee, 0xd4),
            strong: Rgb::new(0x4a, 0xcc, 0x72),
            on_strong: Rgb::new(0x0a, 0x20, 0x12),
        },
    ],
];

pub const ACCENT_NAMES: [&str; 5] = ["青", "粉", "橙", "紫", "绿"];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BasePalette {
    pub text: Rgb,
    pub faint: Rgb,
    pub fresh: Rgb,
    pub card: Rgb,
    pub border: Rgb,
    pub hover: Rgb,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PreviewPalette {
    pub text: Rgb,
    pub faint: Rgb,
    pub fresh: Rgb,
    pub card: Rgb,
    pub border: Rgb,
    pub hover: Rgb,
    pub fill: Rgb,
    pub bar_text: Rgb,
    pub strong: Rgb,
    pub on_strong: Rgb,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MascotPalette {
    pub ink: Rgb,
    pub white: Rgb,
    pub blush: Rgb,
    pub red: Rgb,
    pub red_dark: Rgb,
    pub skin: Rgb,
    pub gold: Rgb,
}

pub fn accent_index(accent: i32) -> usize {
    accent.clamp(0, 4) as usize
}

pub fn accent_name(accent: i32) -> &'static str {
    ACCENT_NAMES[accent_index(accent)]
}

pub fn scheme_kind(theme: i32) -> SchemeKind {
    match theme {
        1 => SchemeKind::Light,
        2 => SchemeKind::Dark,
        _ => SchemeKind::Follow,
    }
}

/// 和 `settings::dark` 一样：1 强制浅色，2 强制深色，其余跟系统。
pub fn effective_dark(theme: i32, system_dark: bool) -> bool {
    match theme {
        1 => false,
        2 => true,
        _ => system_dark,
    }
}

pub fn base_palette(dark: bool) -> BasePalette {
    if dark {
        BasePalette {
            text: Rgb::new(0xf5, 0xf5, 0xf7),
            faint: Rgb::new(0xae, 0xae, 0xb2),
            fresh: Rgb::new(0xff, 0x9f, 0x0a),
            card: Rgb::new(0x2a, 0x2a, 0x2d),
            border: Rgb::new(0x3a, 0x3a, 0x3e),
            hover: Rgb::new(0x33, 0x33, 0x37),
        }
    } else {
        BasePalette {
            text: Rgb::new(0x1d, 0x1d, 0x1f),
            faint: Rgb::new(0x6b, 0x6b, 0x70),
            fresh: Rgb::new(0xff, 0x95, 0x00),
            card: Rgb::new(0xff, 0xff, 0xff),
            border: Rgb::new(0xe3, 0xe6, 0xea),
            hover: Rgb::new(0xe9, 0xeb, 0xee),
        }
    }
}

pub fn preview_palette(accent: i32, dark: bool) -> PreviewPalette {
    let accent = &ACCENTS[accent_index(accent)][usize::from(dark)];
    let base = base_palette(dark);
    PreviewPalette {
        text: base.text,
        faint: base.faint,
        fresh: base.fresh,
        card: base.card,
        border: base.border,
        hover: base.hover,
        fill: accent.fill,
        bar_text: accent.text,
        strong: accent.strong,
        on_strong: accent.on_strong,
    }
}

pub fn accent_strong(accent: i32, dark: bool) -> Rgb {
    ACCENTS[accent_index(accent)][usize::from(dark)].strong
}

pub fn mascot_palette(dark: bool) -> MascotPalette {
    MascotPalette {
        ink: if dark {
            Rgb::new(0xd9, 0xd9, 0xde)
        } else {
            Rgb::new(0x3a, 0x3a, 0x3e)
        },
        white: Rgb::new(0xff, 0xff, 0xff),
        blush: if dark {
            Rgb::new(0xb5, 0x6a, 0x7c)
        } else {
            Rgb::new(0xf5, 0xa8, 0xb8)
        },
        red: Rgb::new(0xd9, 0x4f, 0x4f),
        red_dark: Rgb::new(0xa9, 0x3a, 0x3a),
        skin: Rgb::new(0xff, 0xe3, 0xc9),
        gold: Rgb::new(0xff, 0xd3, 0x4d),
    }
}

/// 候选窗字号：`(基准 × scale / 100)`，整数除法。DPI 由界面库自己乘，这里不再乘。
pub fn scaled_px(base: i32, scale: i32) -> i32 {
    base * scale / 100
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hex_row(accent: i32, dark: bool) -> [String; 4] {
        let a = &ACCENTS[accent as usize][usize::from(dark)];
        [
            a.fill.hex(),
            a.text.hex(),
            a.strong.hex(),
            a.on_strong.hex(),
        ]
    }

    #[test]
    fn five_accents_match_the_ime_table() {
        let table = [
            (0, false, ["#DDF2F6", "#17606C", "#2FB8CC", "#FFFFFF"]),
            (0, true, ["#1E3A44", "#B7E6F0", "#40C8E0", "#06222A"]),
            (1, false, ["#F8E4EC", "#822E54", "#D46A9C", "#FFFFFF"]),
            (1, true, ["#442234", "#F4C8DA", "#E07AB0", "#2A0C18"]),
            (2, false, ["#FAEEE0", "#8A4A1A", "#E09640", "#FFFFFF"]),
            (2, true, ["#44301E", "#F4D6B0", "#E8A04A", "#2A1806"]),
            (3, false, ["#EEE6FA", "#5A3A96", "#9A70D8", "#FFFFFF"]),
            (3, true, ["#32264A", "#D6C6F4", "#A882E4", "#1A102E"]),
            (4, false, ["#E4F4E8", "#26683C", "#3EB060", "#FFFFFF"]),
            (4, true, ["#1E3C2A", "#C8EED4", "#4ACC72", "#0A2012"]),
        ];
        for (accent, dark, expect) in table {
            let got = hex_row(accent, dark);
            assert_eq!(
                got.as_slice(),
                expect.as_slice(),
                "accent {accent} dark {dark}"
            );
        }
    }

    #[test]
    fn base_colors_do_not_follow_accent() {
        let light = base_palette(false);
        let dark = base_palette(true);
        assert_eq!(light.text.hex(), "#1D1D1F");
        assert_eq!(light.faint.hex(), "#6B6B70");
        assert_eq!(light.fresh.hex(), "#FF9500");
        assert_eq!(light.card.hex(), "#FFFFFF");
        assert_eq!(light.border.hex(), "#E3E6EA");
        assert_eq!(light.hover.hex(), "#E9EBEE");
        assert_eq!(dark.text.hex(), "#F5F5F7");
        assert_eq!(dark.faint.hex(), "#AEAEB2");
        assert_eq!(dark.fresh.hex(), "#FF9F0A");
        assert_eq!(dark.card.hex(), "#2A2A2D");
        assert_eq!(dark.border.hex(), "#3A3A3E");
        assert_eq!(dark.hover.hex(), "#333337");

        let cyan = preview_palette(0, false);
        let green = preview_palette(4, false);
        assert_eq!(cyan.card, green.card);
        assert_eq!(cyan.text, green.text);
        assert_eq!(cyan.fill.hex(), "#DDF2F6");
        assert_eq!(green.strong.hex(), "#3EB060");
        assert_eq!(preview_palette(0, true).strong.hex(), "#40C8E0");
    }

    #[test]
    fn dots_use_strong_of_the_preview_scheme() {
        assert_eq!(accent_strong(1, false).hex(), "#D46A9C");
        assert_eq!(accent_strong(3, true).hex(), "#A882E4");
        assert_eq!(accent_name(2), "橙");
    }

    #[test]
    fn theme_mapping_does_not_reuse_slint_enum_numbers() {
        assert_eq!(scheme_kind(0), SchemeKind::Follow);
        assert_eq!(scheme_kind(1), SchemeKind::Light);
        assert_eq!(scheme_kind(2), SchemeKind::Dark);
        assert_eq!(scheme_kind(9), SchemeKind::Follow);
        assert!(!effective_dark(1, true));
        assert!(effective_dark(2, false));
        assert!(effective_dark(0, true));
        assert!(!effective_dark(0, false));
    }

    #[test]
    fn preview_font_pixels_use_integer_division() {
        assert_eq!(scaled_px(TEXT_PX, 90), 14);
        assert_eq!(scaled_px(ANNO_PX, 90), 10);
        assert_eq!(scaled_px(INDEX_PX, 90), 9);
        assert_eq!(scaled_px(TEXT_PX, 100), 16);
        assert_eq!(scaled_px(ANNO_PX, 100), 12);
        assert_eq!(scaled_px(INDEX_PX, 100), 11);
        assert_eq!(scaled_px(TEXT_PX, 115), 18);
        assert_eq!(scaled_px(ANNO_PX, 115), 13);
        assert_eq!(scaled_px(INDEX_PX, 115), 12);
        // 界面档位之外、仍合法的字号也按同一公式，不改写成 90/100/115。
        assert_eq!(scaled_px(TEXT_PX, 80), 12);
        assert_eq!(scaled_px(TEXT_PX, 140), 22);
    }

    #[test]
    fn mascot_colors_match_the_gdi_panel() {
        let light = mascot_palette(false);
        let dark = mascot_palette(true);
        assert_eq!(light.ink.hex(), "#3A3A3E");
        assert_eq!(dark.ink.hex(), "#D9D9DE");
        assert_eq!(light.white.hex(), "#FFFFFF");
        assert_eq!(dark.white.hex(), "#FFFFFF");
        assert_eq!(light.blush.hex(), "#F5A8B8");
        assert_eq!(dark.blush.hex(), "#B56A7C");
        assert_eq!(light.red.hex(), "#D94F4F");
        assert_eq!(dark.red.hex(), "#D94F4F");
        assert_eq!(light.red_dark.hex(), "#A93A3A");
        assert_eq!(light.skin.hex(), "#FFE3C9");
        assert_eq!(light.gold.hex(), "#FFD34D");
        assert_eq!(dark.gold, light.gold);
        assert_eq!(dark.skin, light.skin);
    }

    #[test]
    fn mascot_hex_values_are_the_ones_drawn_in_the_window() {
        let ui = include_str!("../ui/app.slint");
        for dark in [false, true] {
            let mascot = mascot_palette(dark);
            for hex in [
                mascot.ink,
                mascot.white,
                mascot.blush,
                mascot.red,
                mascot.red_dark,
                mascot.skin,
                mascot.gold,
            ] {
                assert!(ui.contains(&hex.hex()), "界面里缺 {}", hex.hex());
            }
        }
    }
}
