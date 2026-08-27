//! Builds `assets/menu-terms.json`: for every ordinary English word, the menu
//! entries that mean roughly the same thing.
//!
//! Menu search matches on spelling by itself. This is what lets it also answer
//! "salary" with "Pays wages". The matching is done here rather than in the
//! page because it takes an embedding model the page has no way to carry: the
//! model is thirty megabytes and its crates need threads and a filesystem.
//!
//!   cargo run --release -- --index ../../assets/menu-index.json
//!
//! Words are scored against every entry once, and each word keeps the few
//! entries it is closest to. A word close to nothing is left out entirely,
//! which is most of them, and is why the table is small enough to ship.

use std::collections::HashMap;
use std::path::PathBuf;

use anyhow::{Context, Result};
use grow::find::{Entry, Index, Terms};
use model2vec_rs::model::StaticModel;

const MODEL: &str = "minishlab/potion-base-8M";

struct Args {
    /// Build the table for the picture slots in the Build panel instead of for
    /// the menus. That list comes from the catalog rather than from a file, so
    /// there is nothing to read in.
    made: bool,
    index: PathBuf,
    out: PathBuf,
    model: String,
    /// Words below this cosine are not worth a row: they would only add noise
    /// to a list the fuzzy matcher already fills.
    threshold: f32,
    /// How many entries one word may point at.
    top: usize,
    words: Option<PathBuf>,
}

fn main() -> Result<()> {
    let args = parse_args()?;

    let entries: Vec<Entry> = if args.made {
        grow::civ::sprites::made_entries()
    } else {
        let raw = std::fs::read_to_string(&args.index)
            .with_context(|| format!("reading {}", args.index.display()))?;
        serde_json::from_str(&raw).context("parsing the menu index")?
    };
    if entries.is_empty() {
        anyhow::bail!("nothing to build a table for; run tools/menuindex.js first");
    }
    let index = Index::new(entries);
    eprintln!("{} entries, stamp {}", index.entries.len(), index.stamp());

    let model = StaticModel::from_pretrained(&args.model, None, None, None)
        .with_context(|| format!("loading {}", args.model))?;

    // What an entry means, in words: its label first, then where it lives and
    // whatever the small print says it is for.
    let texts: Vec<String> = index
        .entries
        .iter()
        .map(|e| {
            [&e.label, &e.group, &e.tab_label, &e.mode_label, &e.hint]
                .iter()
                .filter(|p| !p.is_empty())
                .map(|p| p.as_str())
                .collect::<Vec<_>>()
                .join(". ")
        })
        .collect();
    let entry_vecs: Vec<Vec<f32>> = model.encode(&texts).into_iter().map(normalize).collect();

    let vocab = load_vocab(&args)?;
    eprintln!("scoring {} words", vocab.len());

    let word_vecs = model.encode(&vocab);

    let mut words: HashMap<String, Vec<(u32, u8)>> = HashMap::new();
    let mut kept = 0usize;
    for (word, vec) in vocab.iter().zip(word_vecs) {
        let vec = normalize(vec);
        let mut scored: Vec<(usize, f32)> = entry_vecs
            .iter()
            .enumerate()
            .map(|(i, e)| (i, dot(&vec, e)))
            .filter(|(_, s)| *s >= args.threshold)
            .collect();
        if scored.is_empty() {
            continue;
        }
        scored.sort_by(|a, b| b.1.total_cmp(&a.1));
        scored.truncate(args.top);
        kept += 1;
        words.insert(
            word.clone(),
            scored
                .into_iter()
                .map(|(i, s)| (i as u32, (s.clamp(0.0, 1.0) * 255.0).round() as u8))
                .collect(),
        );
    }
    eprintln!("{kept} words cleared {:.2}", args.threshold);

    let terms = Terms { stamp: index.stamp(), words };
    // Sorted, so rebuilding the same table twice gives the same file and a
    // diff only shows what really changed.
    let mut keys: Vec<&String> = terms.words.keys().collect();
    keys.sort();
    let mut out = String::from("{\n  \"stamp\": ");
    out.push_str(&serde_json::to_string(&terms.stamp)?);
    out.push_str(",\n  \"words\": {\n");
    for (i, key) in keys.iter().enumerate() {
        out.push_str("    ");
        out.push_str(&serde_json::to_string(key)?);
        out.push_str(": ");
        out.push_str(&serde_json::to_string(&terms.words[*key])?);
        if i + 1 < keys.len() {
            out.push(',');
        }
        out.push('\n');
    }
    out.push_str("  }\n}\n");

    std::fs::write(&args.out, &out).with_context(|| format!("writing {}", args.out.display()))?;
    eprintln!("wrote {} ({} KB)", args.out.display(), out.len() / 1024);
    Ok(())
}

/// The words to offer. By default the model's own vocabulary, which is exactly
/// the set of words it has a vector for; anything else is guesswork it would
/// have to spell out of pieces.
fn load_vocab(args: &Args) -> Result<Vec<String>> {
    let path = match &args.words {
        Some(p) => p.clone(),
        None => tokenizer_path(&args.model)?,
    };
    let raw = std::fs::read_to_string(&path)
        .with_context(|| format!("reading {}", path.display()))?;

    let mut words: Vec<String> = if path.extension().is_some_and(|e| e == "json") {
        let doc: serde_json::Value = serde_json::from_str(&raw).context("parsing tokenizer.json")?;
        let vocab = doc
            .pointer("/model/vocab")
            .and_then(|v| v.as_object())
            .context("tokenizer.json has no model.vocab")?;
        vocab.keys().cloned().collect()
    } else {
        raw.lines().map(|l| l.to_string()).collect()
    };

    // Plain lowercase words only. Word pieces, punctuation and capitalized
    // names are not things anybody types into a menu search box.
    words.retain(|w| {
        let n = w.chars().count();
        (3..=14).contains(&n) && w.chars().all(|c| c.is_ascii_lowercase())
    });
    words.sort();
    words.dedup();
    Ok(words)
}

fn tokenizer_path(model: &str) -> Result<PathBuf> {
    let local = PathBuf::from(model).join("tokenizer.json");
    if local.exists() {
        return Ok(local);
    }
    let api = hf_hub::api::sync::Api::new().context("opening the model cache")?;
    api.model(model.to_string())
        .get("tokenizer.json")
        .with_context(|| format!("fetching tokenizer.json for {model}"))
}

fn normalize(mut v: Vec<f32>) -> Vec<f32> {
    let norm = v.iter().map(|x| x * x).sum::<f32>().sqrt();
    if norm > 0.0 {
        for x in v.iter_mut() {
            *x /= norm;
        }
    }
    v
}

fn dot(a: &[f32], b: &[f32]) -> f32 {
    a.iter().zip(b).map(|(x, y)| x * y).sum()
}

fn parse_args() -> Result<Args> {
    let here = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let assets = here.join("../../assets");
    let mut args = Args {
        made: false,
        index: assets.join("menu-index.json"),
        out: assets.join("menu-terms.json"),
        model: MODEL.to_string(),
        threshold: 0.45,
        top: 3,
        words: None,
    };
    let mut it = std::env::args().skip(1);
    while let Some(arg) = it.next() {
        let mut value = || it.next().with_context(|| format!("{arg} wants a value"));
        match arg.as_str() {
            "--made" => {
                args.made = true;
                args.out = assets.join("made-terms.json");
            }
            "--index" => args.index = value()?.into(),
            "--out" => args.out = value()?.into(),
            "--model" => args.model = value()?,
            "--words" => args.words = Some(value()?.into()),
            "--threshold" => args.threshold = value()?.parse().context("--threshold")?,
            "--top" => args.top = value()?.parse().context("--top")?,
            "-h" | "--help" => {
                println!("menu-terms [--made] [--index P] [--out P] [--model ID|DIR] [--words P] [--threshold F] [--top N]");
                std::process::exit(0);
            }
            other => anyhow::bail!("unknown argument {other}"),
        }
    }
    Ok(args)
}
