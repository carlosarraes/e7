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

const REFRESH_BUTTON: &[u8] = include_bytes!("../assets/refresh.png");

/// A template prepared for normalized cross-correlation.
pub struct Pattern {
    pub w: u32,
    pub h: u32,
    /// RGB interleaved, per-channel mean subtracted.
    zero_mean: Vec<f32>,
    norm: f32,
}

impl Pattern {
    pub fn from_png(bytes: &[u8], what: &str) -> Result<Self> {
        let img = image::load_from_memory(bytes)
            .with_context(|| format!("decode {what} template"))?
            .to_rgb8();
        Self::from_image(&img, what)
    }

    pub fn from_image(img: &RgbImage, what: &str) -> Result<Self> {
        let (w, h) = img.dimensions();
        ensure!(w > 0 && h > 0, "empty template for {what}");
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
            w,
            h,
            zero_mean,
            norm,
        })
    }
}

pub struct Template {
    pub item: Item,
    pub pattern: Pattern,
}

impl Template {
    pub fn from_png(item: Item, bytes: &[u8]) -> Result<Self> {
        Ok(Self {
            item,
            pattern: Pattern::from_png(bytes, item.key())?,
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

    pub fn threshold(&self) -> f32 {
        self.threshold
    }

    /// Prepare one screenshot for matching every template against it.
    pub fn scan<'a>(&'a self, screen: &RgbImage) -> Scan<'a> {
        Scan {
            matcher: self,
            strip: Strip::new(screen, self.col.clone(), 0..screen.height()),
        }
    }

    fn template(&self, item: Item) -> Option<&Template> {
        self.templates.iter().find(|t| t.item == item)
    }
}

/// A screenshot's search column, built once and reused across templates.
pub struct Scan<'a> {
    matcher: &'a Matcher,
    strip: Strip,
}

impl Scan<'_> {
    pub fn find(&self, item: Item) -> Option<Hit> {
        self.best(item)
            .filter(|h| h.score >= self.matcher.threshold)
    }

    pub fn best(&self, item: Item) -> Option<Hit> {
        let tpl = self.matcher.template(item)?;
        self.strip
            .best(&tpl.pattern)
            .map(|(x, y, score)| Hit { item, x, y, score })
    }
}

/// A fixed UI element whose presence identifies a screen (the shop's Refresh button).
pub struct Anchor {
    pattern: Pattern,
    xs: Range<u32>,
    ys: Range<u32>,
    threshold: f32,
}

impl Anchor {
    pub fn new(pattern: Pattern, xs: Range<u32>, ys: Range<u32>, threshold: f32) -> Self {
        Self {
            pattern,
            xs,
            ys,
            threshold,
        }
    }

    /// The "Refresh" label on the shop's refresh button, bottom-left of the screen.
    pub fn refresh_button() -> Result<Self> {
        Ok(Self::new(
            Pattern::from_png(REFRESH_BUTTON, "refresh button")?,
            280..540,
            940..1040,
            0.75,
        ))
    }

    pub fn score(&self, screen: &RgbImage) -> f32 {
        Strip::new(screen, self.xs.clone(), self.ys.clone())
            .best(&self.pattern)
            .map(|(_, _, s)| s)
            .unwrap_or(0.0)
    }

    pub fn visible(&self, screen: &RgbImage) -> bool {
        self.score(screen) >= self.threshold
    }
}

/// A rectangular search region as f32 RGB plus per-channel integral tables of
/// sum and sum-of-squares, so each window's variance is O(1) and only the dot
/// product costs O(template).
struct Strip {
    x0: u32,
    y0: u32,
    w: usize,
    h: usize,
    px: Vec<f32>,
    isum: [Vec<f64>; 3],
    isq: [Vec<f64>; 3],
}

impl Strip {
    fn new(screen: &RgbImage, xs: Range<u32>, ys: Range<u32>) -> Self {
        let x0 = xs.start.min(screen.width());
        let x1 = xs.end.min(screen.width());
        let y0 = ys.start.min(screen.height());
        let y1 = ys.end.min(screen.height());
        let w = (x1 - x0) as usize;
        let h = (y1 - y0) as usize;
        let mut px = Vec::with_capacity(w * h * 3);
        for y in y0..y1 {
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
            y0,
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

    /// Best normalized cross-correlation over every valid window position.
    fn best(&self, pat: &Pattern) -> Option<(u32, u32, f32)> {
        let (tw, th) = (pat.w as usize, pat.h as usize);
        if self.w < tw || self.h < th {
            return None;
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
                        let row = &pat.zero_mean[ty * tw * 3..(ty + 1) * tw * 3];
                        num += win.iter().zip(row).map(|(a, b)| a * b).sum::<f32>();
                    }
                    let var: f64 = (0..3)
                        .map(|c| {
                            let s = Self::rect(&self.isum[c], w1, x, y, tw, th);
                            Self::rect(&self.isq[c], w1, x, y, tw, th) - s * s / n
                        })
                        .sum();
                    let score = if var > 1e-6 {
                        num / (var.sqrt() as f32 * pat.norm)
                    } else {
                        0.0
                    };
                    (self.x0 + x as u32, self.y0 + y as u32, score)
                })
            })
            .max_by(|a, b| a.2.total_cmp(&b.2))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture(name: &str) -> RgbImage {
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
                .scan(&fixture(frame))
                .find(item)
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
            let img = fixture(frame);
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
                let best = m.scan(&img).best(item).map(|h| h.score).unwrap_or(0.0);
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
        assert!(m.scan(&screen).find(Item::Fb).is_none());
        paste(&mut screen, 840);
        let hit = m.scan(&screen).find(Item::Fb).unwrap();
        assert_eq!((hit.x, hit.y), (840, 500));
    }

    #[test]
    fn refresh_anchor_separates_shop_from_other_screens() {
        // fixtures are the 280..540 x 940..1040 region of full frames
        let anchor = Anchor::new(
            Pattern::from_png(REFRESH_BUTTON, "refresh").unwrap(),
            0..260,
            0..100,
            0.75,
        );
        let shop = anchor.score(&fixture("anchor_shop"));
        let battle = anchor.score(&fixture("anchor_battle"));
        assert!(shop >= 0.9, "shop {shop}");
        assert!(battle < 0.5, "battle {battle}");
        assert!(anchor.visible(&fixture("anchor_shop")));
        assert!(!anchor.visible(&fixture("anchor_battle")));
    }
}
