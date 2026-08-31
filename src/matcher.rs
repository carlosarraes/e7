use std::ops::Range;
use std::path::Path;

use anyhow::{Context, Result, ensure};
use image::RgbImage;
use rayon::prelude::*;

use crate::item::Item;

pub const SCREEN_W: u32 = 1920;
pub const SCREEN_H: u32 = 1080;

/// Item icons only ever appear in this column of the shop list.
pub fn default_column() -> Range<u32> {
    (SCREEN_W as f64 * 0.42) as u32..(SCREEN_W as f64 * 0.52) as u32
}

pub fn embedded(item: Item) -> &'static [u8] {
    match item {
        Item::Cov => include_bytes!("../assets/cov.png"),
        Item::Mys => include_bytes!("../assets/mys.png"),
        Item::Fb => include_bytes!("../assets/fb.png"),
    }
}

pub struct Template {
    pub item: Item,
    pub w: u32,
    pub h: u32,
    /// RGB interleaved, per-channel mean subtracted.
    zero_mean: Vec<f32>,
    norm: f32,
}

impl Template {
    pub fn from_png(item: Item, bytes: &[u8]) -> Result<Self> {
        let img = image::load_from_memory(bytes)
            .with_context(|| format!("decode {} template", item.key()))?
            .to_rgb8();
        Self::from_image(item, &img)
    }

    pub fn from_image(item: Item, img: &RgbImage) -> Result<Self> {
        let (w, h) = img.dimensions();
        ensure!(w > 0 && h > 0, "empty template for {}", item.key());
        let n = (w * h) as f32;
        let raw = img.as_raw();
        let mut mean = [0f32; 3];
        for px in raw.chunks_exact(3) {
            for (c, m) in mean.iter_mut().enumerate() {
                *m += px[c] as f32;
            }
        }
        for m in &mut mean {
            *m /= n;
        }
        let zero_mean: Vec<f32> = raw
            .iter()
            .enumerate()
            .map(|(i, &v)| v as f32 - mean[i % 3])
            .collect();
        let norm = zero_mean.iter().map(|v| v * v).sum::<f32>().sqrt();
        Ok(Self {
            item,
            w,
            h,
            zero_mean,
            norm,
        })
    }
}

pub fn load_templates(items: &[Item], dir: Option<&Path>) -> Result<Vec<Template>> {
    items
        .iter()
        .map(|&item| match dir {
            Some(d) => {
                let path = d.join(item.asset());
                let bytes =
                    std::fs::read(&path).with_context(|| format!("read {}", path.display()))?;
                Template::from_png(item, &bytes)
            }
            None => Template::from_png(item, embedded(item)),
        })
        .collect()
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Hit {
    pub item: Item,
    pub x: u32,
    pub y: u32,
    pub score: f32,
}

pub struct Matcher {
    templates: Vec<Template>,
    threshold: f32,
    col: Range<u32>,
}

impl Matcher {
    pub fn new(templates: Vec<Template>, threshold: f32, col: Range<u32>) -> Self {
        Self {
            templates,
            threshold,
            col,
        }
    }

    /// Best-scoring position, if it clears the threshold.
    pub fn find(&self, screen: &RgbImage, item: Item) -> Option<Hit> {
        self.best(screen, item)
            .filter(|h| h.score >= self.threshold)
    }

    /// Highest-scoring position regardless of threshold (for diagnostics).
    pub fn best(&self, screen: &RgbImage, item: Item) -> Option<Hit> {
        let tpl = self.template(item)?;
        let strip = Strip::new(screen, &self.col);
        strip
            .scores(tpl)
            .into_iter()
            .max_by(|a, b| a.score.total_cmp(&b.score))
    }

    fn template(&self, item: Item) -> Option<&Template> {
        self.templates.iter().find(|t| t.item == item)
    }
}

/// The searched column as f32 RGB plus per-channel integral tables of sum and
/// sum-of-squares, so each window's variance is O(1) and only the dot product
/// costs O(template).
struct Strip {
    x0: u32,
    w: usize,
    h: usize,
    px: Vec<f32>,
    isum: [Vec<f64>; 3],
    isq: [Vec<f64>; 3],
}

impl Strip {
    fn new(screen: &RgbImage, col: &Range<u32>) -> Self {
        let x0 = col.start.min(screen.width());
        let x1 = col.end.min(screen.width());
        let w = (x1 - x0) as usize;
        let h = screen.height() as usize;
        let mut px = Vec::with_capacity(w * h * 3);
        for y in 0..h as u32 {
            for x in x0..x1 {
                px.extend(screen.get_pixel(x, y).0.iter().map(|&v| v as f32));
            }
        }
        let w1 = w + 1;
        let mut isum = [
            vec![0f64; w1 * (h + 1)],
            vec![0f64; w1 * (h + 1)],
            vec![0f64; w1 * (h + 1)],
        ];
        let mut isq = isum.clone();
        for y in 0..h {
            for x in 0..w {
                for c in 0..3 {
                    let v = px[(y * w + x) * 3 + c] as f64;
                    let i = (y + 1) * w1 + (x + 1);
                    isum[c][i] = v + isum[c][i - 1] + isum[c][i - w1] - isum[c][i - w1 - 1];
                    isq[c][i] = v * v + isq[c][i - 1] + isq[c][i - w1] - isq[c][i - w1 - 1];
                }
            }
        }
        Self {
            x0,
            w,
            h,
            px,
            isum,
            isq,
        }
    }

    fn rect(tab: &[f64], w1: usize, x: usize, y: usize, tw: usize, th: usize) -> f64 {
        tab[(y + th) * w1 + x + tw] - tab[y * w1 + x + tw] - tab[(y + th) * w1 + x]
            + tab[y * w1 + x]
    }

    /// Normalized cross-correlation at every valid window position, row-major.
    fn scores(&self, tpl: &Template) -> Vec<Hit> {
        let (tw, th) = (tpl.w as usize, tpl.h as usize);
        if self.w < tw || self.h < th {
            return Vec::new();
        }
        let n = (tw * th) as f64;
        let w1 = self.w + 1;
        (0..=self.h - th)
            .into_par_iter()
            .flat_map_iter(|y| {
                (0..=self.w - tw).map(move |x| {
                    let mut num = 0f32;
                    for ty in 0..th {
                        let start = ((y + ty) * self.w + x) * 3;
                        let win = &self.px[start..start + tw * 3];
                        let row = &tpl.zero_mean[ty * tw * 3..(ty + 1) * tw * 3];
                        num += win.iter().zip(row).map(|(a, b)| a * b).sum::<f32>();
                    }
                    let var: f64 = (0..3)
                        .map(|c| {
                            let s = Self::rect(&self.isum[c], w1, x, y, tw, th);
                            Self::rect(&self.isq[c], w1, x, y, tw, th) - s * s / n
                        })
                        .sum();
                    let score = if var > 1e-6 {
                        num / (var.sqrt() as f32 * tpl.norm)
                    } else {
                        0.0
                    };
                    Hit {
                        item: tpl.item,
                        x: self.x0 + x as u32,
                        y: y as u32,
                        score,
                    }
                })
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn strip(name: &str) -> RgbImage {
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/");
        image::open(format!("{path}{name}.png")).unwrap().to_rgb8()
    }

    fn matcher() -> Matcher {
        let t = Item::ALL
            .iter()
            .map(|&i| Template::from_png(i, embedded(i)).unwrap())
            .collect();
        Matcher::new(t, 0.75, 0..192)
    }

    const FRAMES: [&str; 6] = [
        "covenant",
        "mystic",
        "friendship",
        "no_item",
        "dialog",
        "mid_swipe",
    ];

    #[test]
    fn finds_each_item_where_opencv_did() {
        let m = matcher();
        for (frame, item, x, y) in [
            ("covenant", Item::Cov, 25, 902),
            ("mystic", Item::Mys, 25, 793),
            ("mystic", Item::Fb, 28, 141),
            ("friendship", Item::Fb, 28, 468),
        ] {
            let hit = m
                .find(&strip(frame), item)
                .unwrap_or_else(|| panic!("{item:?} in {frame}"));
            assert!(hit.score >= 0.9, "{frame}: score {}", hit.score);
            assert!(
                (hit.x as i32 - x).abs() <= 3 && (hit.y as i32 - y).abs() <= 3,
                "{frame}: at {:?}",
                (hit.x, hit.y)
            );
        }
    }

    #[test]
    fn no_false_positives_on_other_frames() {
        let m = matcher();
        for frame in FRAMES {
            let img = strip(frame);
            for item in Item::ALL {
                // mid_swipe holds a half-scrolled mystic medal (scores ~0.87)
                let expected = matches!(
                    (frame, item),
                    ("covenant", Item::Cov)
                        | ("mystic", Item::Mys)
                        | ("mystic", Item::Fb)
                        | ("friendship", Item::Fb)
                        | ("mid_swipe", Item::Mys)
                );
                if expected {
                    continue;
                }
                let best = m.best(&img, item).map(|h| h.score).unwrap_or(0.0);
                assert!(best < 0.7, "{frame}/{item:?}: {best}");
            }
        }
    }

    #[test]
    fn search_is_restricted_to_the_column() {
        let tpl = Template::from_png(Item::Fb, embedded(Item::Fb)).unwrap();
        let mut screen = RgbImage::new(SCREEN_W, SCREEN_H);
        let icon = image::load_from_memory(embedded(Item::Fb))
            .unwrap()
            .to_rgb8();
        let paste = |screen: &mut RgbImage, x0: u32| {
            for (x, y, p) in icon.enumerate_pixels() {
                screen.put_pixel(x0 + x, 500 + y, *p);
            }
        };
        let m = Matcher::new(vec![tpl], 0.75, default_column());
        paste(&mut screen, 100);
        assert!(m.find(&screen, Item::Fb).is_none());
        paste(&mut screen, 840);
        let hit = m.find(&screen, Item::Fb).unwrap();
        assert_eq!((hit.x, hit.y), (840, 500));
    }
}
