//! Furigana split by kanji run (core/src/ruby.rs): each run of kanji gets
//! its own part of the reading, kana in the word get none, found by
//! matching the word's kana against the reading. A split that fails or
//! could go more than one way is the whole reading over the whole word.
//! gates/furigana-placement/goal.md; protected: the model may not edit it.

use ccez_keys_core::{Ruby, ruby_split};

fn split(word: &str, reading: &str) -> Vec<(String, String)> {
    let got: Vec<Ruby> = ruby_split(word.into(), reading.into());
    let joined: String = got.iter().map(|r| r.text.as_str()).collect();
    assert_eq!(joined, word, "the parts of {word} spell the word");
    got.into_iter().map(|r| (r.text, r.rt)).collect()
}

fn parts(p: &[(&str, &str)]) -> Vec<(String, String)> {
    p.iter().map(|(a, b)| (a.to_string(), b.to_string())).collect()
}

fn whole(word: &str, reading: &str) -> Vec<(String, String)> {
    parts(&[(word, reading)])
}

#[test]
fn the_owners_word_okurigana_in_the_middle_and_at_the_end() {
    assert_eq!(split("思い浮かべる", "おもいうかべる"), parts(&[("思", "おも"), ("い", ""), ("浮", "う"), ("かべる", "")]));
}

#[test]
fn okurigana_at_the_end() {
    assert_eq!(split("食べる", "たべる"), parts(&[("食", "た"), ("べる", "")]));
    assert_eq!(split("新しい", "あたらしい"), parts(&[("新", "あたら"), ("しい", "")]));
    assert_eq!(split("勉強する", "べんきょうする"), parts(&[("勉強", "べんきょう"), ("する", "")]));
}

#[test]
fn okurigana_in_the_middle() {
    assert_eq!(split("取り扱い", "とりあつかい"), parts(&[("取", "と"), ("り", ""), ("扱", "あつか"), ("い", "")]));
    assert_eq!(split("申し込む", "もうしこむ"), parts(&[("申", "もう"), ("し", ""), ("込", "こ"), ("む", "")]));
    assert_eq!(split("振り返る", "ふりかえる"), parts(&[("振", "ふ"), ("り", ""), ("返", "かえ"), ("る", "")]));
    assert_eq!(split("落ち着く", "おちつく"), parts(&[("落", "お"), ("ち", ""), ("着", "つ"), ("く", "")]));
    assert_eq!(split("気が付く", "きがつく"), parts(&[("気", "き"), ("が", ""), ("付", "つ"), ("く", "")]));
    assert_eq!(split("引っ越す", "ひっこす"), parts(&[("引", "ひ"), ("っ", ""), ("越", "こ"), ("す", "")]));
}

#[test]
fn kana_before_the_kanji() {
    assert_eq!(split("お茶", "おちゃ"), parts(&[("お", ""), ("茶", "ちゃ")]));
    assert_eq!(split("ご飯", "ごはん"), parts(&[("ご", ""), ("飯", "はん")]));
}

#[test]
fn one_kanji_run_takes_the_whole_reading() {
    // Jukujikun: the reading belongs to the word, not to each kanji, so it
    // stays over the whole of it.
    assert_eq!(split("今日", "きょう"), whole("今日", "きょう"));
    assert_eq!(split("明日", "あした"), whole("明日", "あした"));
    assert_eq!(split("大人", "おとな"), whole("大人", "おとな"));
    assert_eq!(split("人々", "ひとびと"), whole("人々", "ひとびと"));
    assert_eq!(split("一ヶ月", "いっかげつ"), whole("一ヶ月", "いっかげつ"));
    assert_eq!(split("日本語", "にほんご"), whole("日本語", "にほんご"));
}

#[test]
fn a_split_that_could_go_two_ways_falls_back_to_the_whole_word() {
    // もの|の|け or も|の|のけ: no way to tell, so the whole reading, centred.
    assert_eq!(split("物の怪", "もののけ"), whole("物の怪", "もののけ"));
}

#[test]
fn a_reading_that_doesnt_fit_falls_back_to_the_whole_word() {
    assert_eq!(split("食べる", "のむ"), whole("食べる", "のむ"));
    assert_eq!(split("思い浮かべる", "おもいだす"), whole("思い浮かべる", "おもいだす"));
    // Digits or letters in the word: nothing to match them with.
    assert_eq!(split("3日", "みっか"), whole("3日", "みっか"));
}

#[test]
fn words_without_kanji_get_nothing_over_them() {
    assert_eq!(split("ひらがな", "ひらがな"), whole("ひらがな", ""));
    assert_eq!(split("コーヒー", "コーヒー"), whole("コーヒー", ""));
    assert_eq!(split("コーヒー", "こーひー"), whole("コーヒー", ""));
    assert_eq!(split("ありがとう", ""), whole("ありがとう", ""));
}

#[test]
fn no_reading_gets_nothing_over_it() {
    assert_eq!(split("食べる", ""), whole("食べる", ""));
    assert_eq!(split("食べる", "   "), whole("食べる", ""));
}

#[test]
fn katakana_in_the_word_or_the_reading_match_hiragana() {
    assert_eq!(split("消しゴム", "けしごむ"), parts(&[("消", "け"), ("し", ""), ("ゴム", "")]));
    // A reading in katakana: its own letters over the kanji.
    assert_eq!(split("書き", "カキ"), parts(&[("書", "カ"), ("き", "")]));
    assert_eq!(split("漢字", "カンジ"), whole("漢字", "カンジ"));
}

#[test]
fn the_readings_of_a_split_spell_the_reading() {
    for (word, reading) in [("思い浮かべる", "おもいうかべる"), ("取り扱い", "とりあつかい"), ("申し込む", "もうしこむ"), ("お茶", "おちゃ"), ("消しゴム", "けしごむ")] {
        let got = split(word, reading);
        assert!(got.len() > 1, "{word} is split");
        // Kana parts read as themselves (folded to hiragana), kanji parts as their reading.
        let spelled: String = got.iter().map(|(t, rt)| if rt.is_empty() { t.chars().map(|c| if ('\u{30A1}'..='\u{30F6}').contains(&c) { char::from_u32(c as u32 - 0x60).unwrap() } else { c }).collect() } else { rt.clone() }).collect();
        assert_eq!(spelled, reading, "{word}");
        // Every kanji part has a reading; every kana part has none.
        for (t, rt) in &got {
            let kanji = t.chars().any(|c| ('\u{4E00}'..='\u{9FFF}').contains(&c));
            assert_eq!(kanji, !rt.is_empty(), "{word}: {t} / {rt}");
        }
    }
}
