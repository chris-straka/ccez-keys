//! Flags as pictures (Twemoji, CC-BY 4.0): egui can't draw colour emoji.
//! Faded, as on the phone (a bright flag drew the eye).

use std::collections::HashMap;

use eframe::egui::{self, ColorImage, TextureHandle, TextureOptions};

const FLAGS: &[(&str, &[u8])] = &[
    ("ar", include_bytes!("../../assets/flags/ar.png")),
    ("bg", include_bytes!("../../assets/flags/bg.png")),
    ("cs", include_bytes!("../../assets/flags/cs.png")),
    ("de", include_bytes!("../../assets/flags/de.png")),
    ("el", include_bytes!("../../assets/flags/el.png")),
    ("en", include_bytes!("../../assets/flags/en.png")),
    ("es", include_bytes!("../../assets/flags/es.png")),
    ("fa", include_bytes!("../../assets/flags/fa.png")),
    ("fr", include_bytes!("../../assets/flags/fr.png")),
    ("he", include_bytes!("../../assets/flags/he.png")),
    ("hi", include_bytes!("../../assets/flags/hi.png")),
    ("hu", include_bytes!("../../assets/flags/hu.png")),
    ("hy", include_bytes!("../../assets/flags/hy.png")),
    ("id", include_bytes!("../../assets/flags/id.png")),
    ("it", include_bytes!("../../assets/flags/it.png")),
    ("ja", include_bytes!("../../assets/flags/ja.png")),
    ("ko", include_bytes!("../../assets/flags/ko.png")),
    ("nl", include_bytes!("../../assets/flags/nl.png")),
    ("no", include_bytes!("../../assets/flags/no.png")),
    ("pl", include_bytes!("../../assets/flags/pl.png")),
    ("pt", include_bytes!("../../assets/flags/pt.png")),
    ("ro", include_bytes!("../../assets/flags/ro.png")),
    ("ru", include_bytes!("../../assets/flags/ru.png")),
    ("sv", include_bytes!("../../assets/flags/sv.png")),
    ("th", include_bytes!("../../assets/flags/th.png")),
    ("tr", include_bytes!("../../assets/flags/tr.png")),
    ("uk", include_bytes!("../../assets/flags/uk.png")),
    ("vi", include_bytes!("../../assets/flags/vi.png")),
    ("zh", include_bytes!("../../assets/flags/zh.png")),
];

#[derive(Default)]
pub struct Flags {
    made: HashMap<String, Option<TextureHandle>>,
}

fn decode(bytes: &[u8]) -> Option<ColorImage> {
    let mut r = png::Decoder::new(std::io::Cursor::new(bytes)).read_info().ok()?;
    let mut buf = vec![0; r.output_buffer_size()];
    let info = r.next_frame(&mut buf).ok()?;
    let rgba: Vec<u8> = match info.color_type {
        png::ColorType::Rgba => buf[..info.buffer_size()].to_vec(),
        png::ColorType::Rgb => buf[..info.buffer_size()].chunks(3).flat_map(|p| [p[0], p[1], p[2], 255]).collect(),
        _ => return None,
    };
    Some(ColorImage::from_rgba_unmultiplied([info.width as usize, info.height as usize], &rgba))
}

impl Flags {
    /// `lang`'s flag, made once.
    pub fn get(&mut self, ctx: &egui::Context, lang: &str) -> Option<TextureHandle> {
        let base = lang.split('-').next().unwrap_or(lang).to_string();
        self.made
            .entry(base.clone())
            .or_insert_with(|| {
                let bytes = FLAGS.iter().find(|(l, _)| *l == base)?.1;
                Some(ctx.load_texture(format!("flag-{base}"), decode(bytes)?, TextureOptions::LINEAR))
            })
            .clone()
    }
}
