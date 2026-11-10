# Credits

ccez-keys is MIT (LICENSE). It ships data from:

- **wordfreq** v3.2 by Robyn Speer, CC BY-SA 4.0: the word lists in
  `data/words/` (most common words per language). https://github.com/rspeer/wordfreq
- **Wiktionary** through kaikki.org, CC BY-SA 4.0: German capitals and ß
  in `data/words/de.txt`. https://kaikki.org
- **FrequencyWords** by Hermit Dave, from OpenSubtitles 2018, CC BY-SA 4.0:
  `data/words/th.txt` and `hy.txt`. https://github.com/hermitdave/FrequencyWords
- **JMdict** (EDRDG) and **CC-CEDICT**, CC BY-SA 4.0: the Japanese and Chinese
  converters, built on the phone from ccez-lang-core's dictionaries.

## Next words

`data/next/<lang>.txt`: which words tend to follow which, counted by
`keys-next` from [Tatoeba](https://tatoeba.org)'s sentences (CC BY 2.0 FR).
Tatoeba's stock names (Tom, Mary and others) are left out.


## Desktop app (Linux and Windows)

- Translation models: Mozilla's [Firefox Translations](https://github.com/mozilla/firefox-translations-models)
  models (MPL-2.0), unchanged, served from this repo's `models-firefox-1`
  release (`scripts/desktop-models.py`), run by
  [fxtranslate](https://github.com/gregtatum/translations) (MPL-2.0).
- Flags in `desktop/assets/flags/`: [Twemoji](https://github.com/jdecked/twemoji)
  graphics (CC-BY 4.0), copyright Twitter, Inc. and other contributors.
