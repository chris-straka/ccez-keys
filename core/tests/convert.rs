//! Typing Japanese (romaji → kana → words) and Chinese (pinyin → words),
//! on the real dictionaries when this machine has them (ccez-tube's assets).

use ccez_keys_core::convert::{Index, build_chinese, build_japanese, romaji_to_kana};
use std::path::Path;

const DICT: &str = "/Users/c/SWE/ccez-tube/android/app/src/main/assets/dict";

#[test]
fn romaji_become_kana() {
    let k = |s: &str| romaji_to_kana(s);
    assert_eq!(k("kyouhaiitenkidesune"), ("きょうはいいてんきですね".into(), String::new()));
    assert_eq!(k("gakkou"), ("がっこう".into(), String::new()));
    assert_eq!(k("konnichiha"), ("こんにちは".into(), String::new()));
    assert_eq!(k("shinbun"), ("しんぶ".into(), "n".into()));
    assert_eq!(ccez_keys_core::convert::romaji("shinbun", true), ("しんぶん".into(), String::new()));
    assert_eq!(k("kanji"), ("かんじ".into(), String::new()));
    assert_eq!(k("kyout"), ("きょう".into(), "t".into()));
    assert_eq!(k("ra-men"), ("らーめ".into(), "n".into()));
    assert_eq!(k("matcha"), ("まっちゃ".into(), String::new()));
}

fn freq(lang: &str) -> String {
    std::fs::read_to_string(format!("{}/../data/words/{lang}.txt", env!("CARGO_MANIFEST_DIR"))).unwrap()
}

#[test]
fn japanese_words_from_kana() {
    let dict = Path::new(DICT).join("ja.tsv.gz");
    if !dict.exists() {
        return;
    }
    let out = std::env::temp_dir().join("ccez-keys-convert-ja.tsv");
    assert!(build_japanese(&dict, &freq("ja"), &out).unwrap() > 10_000);
    let i = Index::load(&out).unwrap();
    let top = |kana: &str| i.convert(kana, true).first().map(|c| c.text.clone()).unwrap_or_default();
    assert_eq!(top("あした"), "明日");
    assert_eq!(top("がっこう"), "学校");
    assert_eq!(top("たべる"), "食べる");
    // A start of the input: 今日 for きょう, the rest left for later.
    let c = i.convert("きょうは", true);
    assert!(c.iter().any(|x| x.text == "今日" && x.consumed == 3), "{c:?}");
}

#[test]
fn chinese_words_from_pinyin() {
    let dict = Path::new(DICT).join("cedict.txt.gz");
    if !dict.exists() {
        return;
    }
    let out = std::env::temp_dir().join("ccez-keys-convert-zh.tsv");
    assert!(build_chinese(&dict, &freq("zh"), &out).unwrap() > 10_000);
    let i = Index::load(&out).unwrap();
    let top = |p: &str| i.convert(p, false).first().map(|c| c.text.clone()).unwrap_or_default();
    assert_eq!(top("nihao"), "你好");
    assert_eq!(top("xiexie"), "谢谢");
    assert_eq!(top("zhongguo"), "中国");
    let c = i.convert("wohenhao", false);
    assert!(c.iter().any(|x| x.text == "我" && x.consumed == 2), "{c:?}");
    assert!(c.first().is_some_and(|x| !x.reading.is_empty()), "pinyin shown with tone marks: {c:?}");
}
