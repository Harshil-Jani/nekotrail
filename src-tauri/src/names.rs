//! Workspaces are named after cat breeds from around the world. Each new workspace
//! "discovers" a breed and adds it to your Breedex.

use rand::seq::IndexedRandom;
use serde::Serialize;

#[derive(Debug, Clone, Copy, Serialize, PartialEq)]
pub struct Breed {
    /// Slug used for the folder and branch, e.g. `norwegian-forest`.
    pub slug: &'static str,
    pub name: &'static str,
    pub origin: &'static str,
    pub fact: &'static str,
}

const fn b(slug: &'static str, name: &'static str, origin: &'static str, fact: &'static str) -> Breed {
    Breed { slug, name, origin, fact }
}

pub const BREEDS: &[Breed] = &[
    b("abyssinian", "Abyssinian", "Named for Ethiopia", "Named after Abyssinia (now Ethiopia), though genetic studies point to the Indian Ocean coast."),
    b("aegean", "Aegean", "Greece", "A natural breed from the Cycladic islands, often found around fishing ports."),
    b("american-curl", "American Curl", "USA", "Every American Curl descends from one curled-eared stray found in California in 1981."),
    b("american-shorthair", "American Shorthair", "USA", "Descends from cats that sailed with early settlers to keep ships free of mice."),
    b("australian-mist", "Australian Mist", "Australia", "The first cat breed developed in Australia."),
    b("balinese", "Balinese", "USA", "A long-haired Siamese, named for the grace of Balinese dancers rather than for Bali itself."),
    b("bengal", "Bengal", "USA", "Developed by crossing domestic cats with the Asian leopard cat."),
    b("birman", "Birman", "Myanmar", "Every Birman has white gloves on all four paws."),
    b("bombay", "Bombay", "USA", "Bred in Kentucky in the 1950s to look like a miniature black panther."),
    b("british-shorthair", "British Shorthair", "United Kingdom", "One of the oldest English breeds, known for its round face and dense blue-grey coat."),
    b("burmese", "Burmese", "Myanmar", "Modern Burmese trace back to one cat, Wong Mau, brought to San Francisco in 1930."),
    b("burmilla", "Burmilla", "United Kingdom", "Began with an accidental 1981 pairing of a Burmese and a Chinchilla Persian."),
    b("chartreux", "Chartreux", "France", "A French blue cat linked in legend to the Carthusian monks."),
    b("chausie", "Chausie", "USA", "Has jungle cat ancestry, which shows in its long legs and big ears."),
    b("cornish-rex", "Cornish Rex", "United Kingdom", "Has only a soft undercoat, which is why its fur is short and curly."),
    b("cymric", "Cymric", "Isle of Man", "A long-haired Manx."),
    b("devon-rex", "Devon Rex", "United Kingdom", "Big ears and a wavy coat; first found in Devon in 1960."),
    b("egyptian-mau", "Egyptian Mau", "Egypt", "One of the few naturally spotted domestic breeds."),
    b("exotic-shorthair", "Exotic Shorthair", "USA", "A Persian with a short, plush coat."),
    b("havana-brown", "Havana Brown", "United Kingdom", "Named for its rich brown coat, the colour of a Havana cigar."),
    b("himalayan", "Himalayan", "USA", "A Persian with the colour points of a Siamese."),
    b("japanese-bobtail", "Japanese Bobtail", "Japan", "Its short pom-pom tail is different on every cat."),
    b("khao-manee", "Khao Manee", "Thailand", "An all-white Thai cat whose name means white gem; many have odd-coloured eyes."),
    b("korat", "Korat", "Thailand", "A silver-blue cat seen as a sign of good luck in Thailand."),
    b("kurilian-bobtail", "Kurilian Bobtail", "Kuril Islands", "Comes from the Kuril Islands and has a naturally short, kinked tail."),
    b("laperm", "LaPerm", "USA", "Many LaPerm kittens are born bald and grow their curls later."),
    b("lykoi", "Lykoi", "USA", "Its patchy coat gives it a werewolf look; the name comes from the Greek for wolf."),
    b("maine-coon", "Maine Coon", "USA", "The official state cat of Maine."),
    b("manx", "Manx", "Isle of Man", "Many Manx cats are born with no tail at all."),
    b("nebelung", "Nebelung", "USA", "Its name means creature of the mist in German."),
    b("norwegian-forest", "Norwegian Forest Cat", "Norway", "Has a water-resistant double coat built for Scandinavian winters."),
    b("ocicat", "Ocicat", "USA", "Looks wild but has no wild ancestry: it was bred from Abyssinians and Siamese."),
    b("oriental-shorthair", "Oriental Shorthair", "United Kingdom", "A Siamese body in hundreds of colours and patterns."),
    b("persian", "Persian", "Iran", "Brought to Europe from Persia in the 1600s."),
    b("pixiebob", "Pixiebob", "USA", "Many Pixiebobs have extra toes."),
    b("ragdoll", "Ragdoll", "USA", "Named for the way many go limp when picked up."),
    b("russian-blue", "Russian Blue", "Russia", "Said to come from the port of Arkhangelsk."),
    b("savannah", "Savannah", "USA", "A cross between a domestic cat and the African serval."),
    b("scottish-fold", "Scottish Fold", "Scotland", "All Scottish Folds trace back to one barn cat, Susie, found in 1961."),
    b("selkirk-rex", "Selkirk Rex", "USA", "Named after the Selkirk Mountains in Montana."),
    b("siamese", "Siamese", "Thailand", "Its darker points grow where the body is coolest: ears, face, paws and tail."),
    b("siberian", "Siberian", "Russia", "A national cat of Russia with a thick triple coat."),
    b("singapura", "Singapura", "Singapore", "One of the smallest cat breeds."),
    b("sokoke", "Sokoke", "Kenya", "Comes from the Arabuko-Sokoke Forest on Kenya's coast."),
    b("somali", "Somali", "USA", "A long-haired Abyssinian, nicknamed the fox cat for its bushy tail."),
    b("sphynx", "Sphynx", "Canada", "Not truly hairless: most have a fine peach-fuzz coat."),
    b("tonkinese", "Tonkinese", "Canada", "A cross of the Siamese and the Burmese."),
    b("toyger", "Toyger", "USA", "Bred to look like a tiny tiger."),
    b("turkish-angora", "Turkish Angora", "Turkey", "One of the oldest long-haired breeds, from the Ankara region."),
    b("turkish-van", "Turkish Van", "Turkey", "Known as the swimming cat; it comes from the Lake Van region."),
];

/// The breed behind a workspace name like `bengal` or `bengal-2`.
pub fn breed_for(name: &str) -> Option<&'static Breed> {
    let base = match name.rsplit_once('-') {
        Some((head, tail)) if tail.chars().all(|c| c.is_ascii_digit()) => head,
        _ => name,
    };
    BREEDS.iter().find(|b| b.slug == base)
}

/// Prefer a breed you have never discovered; once all are found, reuse one with a number.
pub fn pick(taken: &[String]) -> String {
    let undiscovered: Vec<&Breed> = BREEDS
        .iter()
        .filter(|b| !taken.iter().any(|t| breed_for(t).map(|x| x.slug) == Some(b.slug)))
        .collect();
    if let Some(b) = undiscovered.choose(&mut rand::rng()) {
        return b.slug.to_string();
    }
    let base = BREEDS.choose(&mut rand::rng()).unwrap().slug;
    (2..).map(|i| format!("{base}-{i}")).find(|n| !taken.contains(n)).unwrap()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slugs_are_unique_and_branch_safe() {
        for (i, a) in BREEDS.iter().enumerate() {
            assert!(a.slug.chars().all(|c| c.is_ascii_lowercase() || c == '-'), "{}", a.slug);
            assert!(BREEDS[i + 1..].iter().all(|b| b.slug != a.slug), "duplicate {}", a.slug);
        }
    }

    #[test]
    fn picks_undiscovered_then_numbers() {
        let taken: Vec<String> = BREEDS.iter().skip(1).map(|b| b.slug.to_string()).collect();
        assert_eq!(pick(&taken), BREEDS[0].slug);
        let all: Vec<String> = BREEDS.iter().map(|b| b.slug.to_string()).collect();
        let n = pick(&all);
        assert!(n.ends_with("-2"), "{n}");
        assert_eq!(breed_for(&n).map(|b| b.slug), Some(n.trim_end_matches("-2")));
    }

    #[test]
    fn imported_city_names_have_no_breed() {
        assert!(breed_for("tunis-v1").is_none());
        assert_eq!(breed_for("maine-coon").unwrap().name, "Maine Coon");
    }
}
