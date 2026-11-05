//! The night-sky quest. Every workspace is named after a star in one of eight cat
//! constellations. Creating a workspace lights its star; merging its PR makes it shine,
//! and lines appear between neighbouring stars once both have shipped.

use rand::seq::IndexedRandom;
use serde::Serialize;

#[derive(Debug, Clone, Copy, Serialize, PartialEq)]
pub struct Star {
    /// Used for the folder and branch, e.g. `mochi`.
    pub slug: &'static str,
    pub name: &'static str,
    pub constellation: &'static str,
    /// Position on the sky map, 0..1200 x 0..640.
    pub x: u16,
    pub y: u16,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq)]
pub struct Constellation {
    pub slug: &'static str,
    pub name: &'static str,
    pub story: &'static str,
    /// Lines between stars, as star slugs.
    pub lines: &'static [(&'static str, &'static str)],
}

const fn s(slug: &'static str, name: &'static str, constellation: &'static str, x: u16, y: u16) -> Star {
    Star { slug, name, constellation, x, y }
}

pub const STARS: &[Star] = &[
    // The Loaf: a cat tucked into a perfect rectangle.
    s("mochi", "Mochi", "loaf", 90, 120), s("biscuit", "Biscuit", "loaf", 130, 70), s("tofu", "Tofu", "loaf", 175, 115),
    s("dumpling", "Dumpling", "loaf", 270, 120), s("pudding", "Pudding", "loaf", 270, 210), s("muffin", "Muffin", "loaf", 90, 210),
    // The Pounce: mid-leap, front paws out.
    s("ziggy", "Ziggy", "pounce", 380, 200), s("comet", "Comet", "pounce", 430, 140), s("dash", "Dash", "pounce", 500, 110),
    s("pepper", "Pepper", "pounce", 575, 80), s("rocket", "Rocket", "pounce", 540, 170), s("flick", "Flick", "pounce", 340, 120),
    // The Curled Tail: one long spiral.
    s("nimbus", "Nimbus", "tail", 700, 60), s("swirl", "Swirl", "tail", 760, 90), s("noodle", "Noodle", "tail", 780, 160),
    s("ribbon", "Ribbon", "tail", 730, 200), s("curly", "Curly", "tail", 690, 160), s("twirl", "Twirl", "tail", 725, 130),
    // The Whiskers: three lines either side of a nose.
    s("whisker", "Whisker", "whiskers", 960, 130), s("scritch", "Scritch", "whiskers", 880, 90), s("tickle", "Tickle", "whiskers", 870, 140),
    s("feather", "Feather", "whiskers", 885, 190), s("wisp", "Wisp", "whiskers", 1050, 95), s("bristle", "Bristle", "whiskers", 1055, 185),
    // The Stretch: front legs forward, back arched.
    s("snooze", "Snooze", "stretch", 80, 420), s("dozy", "Dozy", "stretch", 150, 380), s("mellow", "Mellow", "stretch", 230, 330),
    s("lazy", "Lazy", "stretch", 300, 360), s("sunny", "Sunny", "stretch", 330, 440), s("cozy", "Cozy", "stretch", 200, 450),
    // The Kitten: small, round, all head.
    s("pip", "Pip", "kitten", 430, 350), s("button", "Button", "kitten", 470, 320), s("bean", "Bean", "kitten", 510, 350),
    s("peanut", "Peanut", "kitten", 510, 410), s("pebble", "Pebble", "kitten", 430, 410), s("tiny", "Tiny", "kitten", 470, 450),
    // The Yarn Ball: a loop with a loose thread.
    s("yarn", "Yarn", "yarn", 640, 360), s("knit", "Knit", "yarn", 700, 320), s("purl", "Purl", "yarn", 760, 360),
    s("skein", "Skein", "yarn", 740, 430), s("tangle", "Tangle", "yarn", 660, 430), s("loop", "Loop", "yarn", 830, 470),
    // Felis: a sitting cat, after Lalande's 1799 chart.
    s("lalande", "Lalande", "felis", 900, 300), s("midnight", "Midnight", "felis", 930, 260), s("shadow", "Shadow", "felis", 960, 300),
    s("smokey", "Smokey", "felis", 950, 380), s("sable", "Sable", "felis", 1000, 450), s("inky", "Inky", "felis", 900, 450),
    s("ember", "Ember", "felis", 1060, 420), s("onyx", "Onyx", "felis", 1110, 360),
];

pub const CONSTELLATIONS: &[Constellation] = &[
    Constellation { slug: "loaf", name: "The Loaf", story: "A cat tucked into a perfect rectangle, paws hidden.",
        lines: &[("biscuit", "mochi"), ("biscuit", "tofu"), ("tofu", "dumpling"), ("dumpling", "pudding"), ("pudding", "muffin"), ("muffin", "mochi")] },
    Constellation { slug: "pounce", name: "The Pounce", story: "Mid-leap, front paws out, eyes on a bug in the code.",
        lines: &[("flick", "ziggy"), ("ziggy", "comet"), ("comet", "dash"), ("dash", "pepper"), ("dash", "rocket")] },
    Constellation { slug: "tail", name: "The Curled Tail", story: "One long spiral. Follow it inward and you find a nap.",
        lines: &[("nimbus", "swirl"), ("swirl", "noodle"), ("noodle", "ribbon"), ("ribbon", "curly"), ("curly", "twirl")] },
    Constellation { slug: "whiskers", name: "The Whiskers", story: "Six lines that can tell if a gap is wide enough.",
        lines: &[("whisker", "scritch"), ("whisker", "tickle"), ("whisker", "feather"), ("whisker", "wisp"), ("whisker", "bristle")] },
    Constellation { slug: "stretch", name: "The Stretch", story: "Front legs forward, back arched: the first thing after any nap.",
        lines: &[("snooze", "dozy"), ("dozy", "mellow"), ("mellow", "lazy"), ("lazy", "sunny"), ("sunny", "cozy"), ("cozy", "snooze")] },
    Constellation { slug: "kitten", name: "The Kitten", story: "Small, round and mostly head.",
        lines: &[("pip", "button"), ("button", "bean"), ("bean", "peanut"), ("peanut", "tiny"), ("tiny", "pebble"), ("pebble", "pip")] },
    Constellation { slug: "yarn", name: "The Yarn Ball", story: "A loop with one loose thread, which is all it takes.",
        lines: &[("yarn", "knit"), ("knit", "purl"), ("purl", "skein"), ("skein", "tangle"), ("tangle", "yarn"), ("skein", "loop")] },
    Constellation { slug: "felis", name: "Felis", story: "Astronomer Jérôme Lalande drew a cat among the stars in 1799. It was left out when the 88 official constellations were fixed in the 1920s.",
        lines: &[("midnight", "lalande"), ("midnight", "shadow"), ("lalande", "smokey"), ("shadow", "smokey"), ("smokey", "inky"), ("smokey", "sable"), ("sable", "ember"), ("ember", "onyx")] },
];

/// The star behind a workspace name like `mochi` or `mochi-2`.
pub fn star_for(name: &str) -> Option<&'static Star> {
    let base = match name.rsplit_once('-') {
        Some((head, tail)) if tail.chars().all(|c| c.is_ascii_digit()) => head,
        _ => name,
    };
    STARS.iter().find(|s| s.slug == base)
}

/// Prefer a star nobody has lit, starting in the constellation closest to complete,
/// so the sky fills one cat at a time. Once every star is lit, reuse one with a number.
pub fn pick(taken: &[String]) -> String {
    let lit = |slug: &str| taken.iter().any(|t| star_for(t).map(|s| s.slug) == Some(slug));
    let progress = |c: &str| STARS.iter().filter(|s| s.constellation == c && lit(s.slug)).count();
    let unlit: Vec<&Star> = STARS.iter().filter(|s| !lit(s.slug)).collect();
    if let Some(best) = unlit.iter().map(|s| progress(s.constellation)).max() {
        let pool: Vec<&&Star> = unlit.iter().filter(|s| progress(s.constellation) == best).collect();
        // Within the leading constellation, keep its order so the shape grows outward.
        let first = pool[0].constellation;
        let in_first: Vec<&&Star> = pool.iter().copied().filter(|s| s.constellation == first).collect();
        return if best == 0 { pool.choose(&mut rand::rng()).unwrap().slug.to_string() } else { in_first[0].slug.to_string() };
    }
    let base = STARS.choose(&mut rand::rng()).unwrap().slug;
    (2..).map(|i| format!("{base}-{i}")).find(|n| !taken.contains(n)).unwrap()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stars_and_lines_are_consistent() {
        for (i, a) in STARS.iter().enumerate() {
            assert!(a.slug.chars().all(|c| c.is_ascii_lowercase()), "{}", a.slug);
            assert!(STARS[i + 1..].iter().all(|b| b.slug != a.slug), "duplicate {}", a.slug);
            assert!(CONSTELLATIONS.iter().any(|c| c.slug == a.constellation), "{} has no constellation", a.slug);
            assert!(a.x <= 1200 && a.y <= 640);
        }
        for c in CONSTELLATIONS {
            for (a, b) in c.lines {
                for s in [a, b] {
                    let star = star_for(s).unwrap_or_else(|| panic!("line to unknown star {s}"));
                    assert_eq!(star.constellation, c.slug, "{s} drawn in the wrong constellation");
                }
            }
        }
        assert_eq!(STARS.len(), 50);
    }

    #[test]
    fn fills_one_constellation_at_a_time() {
        let first = pick(&[]);
        let c = star_for(&first).unwrap().constellation;
        let second = pick(std::slice::from_ref(&first));
        assert_eq!(star_for(&second).unwrap().constellation, c);
        assert_ne!(second, first);
    }

    #[test]
    fn numbers_once_the_sky_is_full() {
        let all: Vec<String> = STARS.iter().map(|s| s.slug.to_string()).collect();
        let n = pick(&all);
        assert!(n.ends_with("-2"), "{n}");
        assert!(star_for(&n).is_some());
        assert!(star_for("tunis-v1").is_none());
    }
}
