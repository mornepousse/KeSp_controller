//! Touch typing practice on the keyboard's own keymap.
//!
//! The method is keybr.com's, as its README describes it: start with a small set
//! of the language's most frequent letters, add the next one once the current ones
//! reach the target speed, and give the weakest letter more room. What changes
//! here: the letters come from what this keymap can type, and each one is shown on
//! its own key, with its finger and the layer key to hold (TypingClub shows the
//! hand on every key).

use crate::checks::{self, Key};
use crate::keys;
use crate::practice_game::{Game, LineFacts, Reward};
use crate::protocol::layout_remap::KeyboardLayout;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Letters in play at the start, like keybr's "small set".
pub const START_LETTERS: usize = 6;
pub const DEFAULT_TARGET_WPM: u32 = 30;
/// One word is 5 characters, the usual convention for words per minute.
pub const CHARS_PER_WORD: f32 = 5.0;
/// Presses of a letter before its speed counts.
pub const MIN_SAMPLES: u32 = 10;
/// Share of misses a letter may have and still count as known.
pub const MAX_ERROR_RATE: f32 = 0.05;
/// Weight of the newest press in a letter's running averages.
const AVERAGE_WEIGHT: f32 = 0.1;
/// A longer gap is a pause, not the time a key took.
pub const PAUSE_MS: f32 = 2000.0;
/// Pairs trained at once in the Bigrams mode.
pub const WEAK_PAIRS: usize = 5;
/// Times a pair is typed before its speed counts.
pub const PAIR_SAMPLES: u32 = 3;
/// Length of a sprint.
pub const SPRINT_SECONDS: f64 = 60.0;
/// Speeds kept for the chart, per mode.
pub const HISTORY_LEN: usize = 60;
/// Length of a practice line, in characters.
pub const LINE_CHARS: usize = 48;
/// Below this many real words that fit, made-up words fill the line.
const MIN_REAL_WORDS: usize = 12;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Lang {
    English,
    French,
}

impl Lang {
    pub const ALL: [Lang; 2] = [Lang::English, Lang::French];

    pub fn name(self) -> &'static str {
        match self {
            Lang::English => "English",
            Lang::French => "Français",
        }
    }

    pub fn code(self) -> &'static str {
        match self {
            Lang::English => "en",
            Lang::French => "fr",
        }
    }

    pub fn from_code(code: &str) -> Lang {
        Lang::ALL.into_iter().find(|l| l.code() == code).unwrap_or(Lang::English)
    }

    fn words(self) -> &'static str {
        match self {
            Lang::English => include_str!("practice_words/en.txt"),
            Lang::French => include_str!("practice_words/fr.txt"),
        }
    }

    /// Letters from the most to the least frequent in running text.
    fn frequency(self) -> &'static str {
        match self {
            Lang::English => "etaoinshrdlcumwfgypbvkjxqz",
            Lang::French => "esaitnrulodcpmévqfbghjàxèyêzwçùkœ",
        }
    }
}

/// Where a character is typed: its key, and the base-layer key to hold when it
/// lives on another layer.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Place {
    pub layer: usize,
    pub row: usize,
    pub col: usize,
    pub hold: Option<(usize, usize)>,
}

/// The character a key types with no modifier, read from its label on the
/// computer's layout. A layer-tap or mod-tap key counts by its tap.
pub fn char_of(code: u16, layout: &KeyboardLayout) -> Option<char> {
    let tap = match code {
        0x04..=0xFF => code,
        0x4000..=0x5FFF => code & 0xFF,
        // A shifted key counts when the layout gives it one sign it can vouch for:
        // "!" for Shift+1 on QWERTY. Elsewhere it reads "Shift+1", and is left out.
        0x8200..=0x82FF => {
            let label = crate::protocol::layout_remap::shifted_label(layout, (code & 0xFF) as u8);
            let mut chars = label.chars();
            return match (chars.next(), chars.next()) {
                (Some(c), None) => Some(c),
                _ => None,
            };
        }
        _ => return None,
    };
    if tap < 0x04 {
        return None;
    }
    let label = keys::label_for(tap, layout);
    if label == "Space" {
        return Some(' ');
    }
    // AZERTY labels carry both signs, "é 2": the first one is the unshifted one.
    let first = label.split(' ').next()?;
    let mut chars = first.chars();
    let c = chars.next()?;
    if chars.next().is_some() {
        return None;
    }
    c.to_lowercase().next()
}

/// Every character the keymap can type, on the lowest layer that has it. A layer
/// no base-layer key holds is left out: nothing reaches it while typing.
pub fn places(keys: &[Key], layout: &KeyboardLayout) -> BTreeMap<char, Place> {
    let mut sorted: Vec<&Key> = keys.iter().collect();
    sorted.sort_by_key(|k| (k.layer, k.row, k.col));
    let mut out = BTreeMap::new();
    for k in sorted {
        let Some(c) = char_of(k.code, layout) else { continue };
        if out.contains_key(&c) {
            continue;
        }
        let hold = if k.layer == 0 {
            None
        } else {
            // A layer-mod key (LM) adds a modifier: a sign typed under it would leave as a
            // shortcut. A plain hold (MO, LT, OSL) comes first.
            let holds = |b: &&Key| b.layer == 0 && checks::layer_target(b.code) == Some((k.layer, false));
            let holder = keys
                .iter()
                .filter(holds)
                .min_by_key(|b| (0x7000..=0x7FFF).contains(&b.code));
            match holder {
                Some(b) => Some((b.row, b.col)),
                None => continue,
            }
        };
        out.insert(c, Place { layer: k.layer, row: k.row, col: k.col, hold });
    }
    out
}

/// Signs, digits and punctuation this keymap can type: everything but letters and space.
pub fn symbols(places: &BTreeMap<char, Place>) -> Vec<char> {
    places.keys().copied().filter(|c| !c.is_alphabetic() && *c != ' ').collect()
}

/// What a line trains.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Mode {
    /// Letters one at a time, as on keybr.com.
    Letters,
    /// Real words with every letter, to keep the pace.
    Words,
    /// The keymap's signs and digits, with their layer keys.
    Symbols,
    /// 60 seconds of real words, against the clock and a record.
    Sprint,
    /// The slowest letter pairs, as on ngram-type.
    Bigrams,
}

impl Mode {
    pub const ALL: [Mode; 5] = [Mode::Letters, Mode::Words, Mode::Bigrams, Mode::Symbols, Mode::Sprint];

    pub fn name(self) -> &'static str {
        match self {
            Mode::Letters => "Letters",
            Mode::Words => "Words",
            Mode::Symbols => "Symbols",
            Mode::Sprint => "Sprint",
            Mode::Bigrams => "Bigrams",
        }
    }

    pub fn code(self) -> &'static str {
        match self {
            Mode::Letters => "letters",
            Mode::Words => "words",
            Mode::Symbols => "symbols",
            Mode::Sprint => "sprint",
            Mode::Bigrams => "bigrams",
        }
    }

    pub fn from_code(code: &str) -> Mode {
        Mode::ALL.into_iter().find(|m| m.code() == code).unwrap_or(Mode::Letters)
    }
}

/// Real words holding the pairs to train; the pair alone when no word has it.
pub fn bigram_line(lang: Lang, letters: &[char], pairs: &[String], rng: &mut Rng) -> String {
    let words: Vec<&str> = lang.words().split_whitespace().filter(|w| w.chars().all(|c| letters.contains(&c))).collect();
    let mut out = String::new();
    if pairs.is_empty() {
        return out;
    }
    while out.chars().count() < LINE_CHARS {
        let pair = &pairs[rng.below(pairs.len())];
        let with: Vec<&str> = words.iter().copied().filter(|w| w.contains(pair.as_str())).collect();
        let word = if with.is_empty() { pair.repeat(2) } else { with[rng.below(with.len())].to_string() };
        if !out.is_empty() {
            out.push(' ');
        }
        out.push_str(&word);
    }
    out
}

/// Groups of 2 to 4 signs, the focus one in about 3 out of 10.
pub fn symbol_line(symbols: &[char], focus: Option<char>, rng: &mut Rng) -> String {
    let mut out = String::new();
    if symbols.is_empty() {
        return out;
    }
    while out.chars().count() < LINE_CHARS {
        if !out.is_empty() {
            out.push(' ');
        }
        for _ in 0..2 + rng.below(3) {
            let c = match focus {
                Some(f) if rng.below(10) < 3 => f,
                _ => symbols[rng.below(symbols.len())],
            };
            out.push(c);
        }
    }
    out
}

/// The key that gives capitals: a plain Shift first, then a one-shot Shift, then a
/// mod-tap with Shift (a home-row mod), on the base layer.
pub fn shift_key(keys: &[Key]) -> Option<(usize, usize)> {
    let rank = |code: u16| match code {
        0xE1 | 0xE5 => Some(0),
        0x3000..=0x30FF if code & 0x22 != 0 => Some(1),
        0x5000..=0x5FFF if (code >> 8) & 0x02 != 0 => Some(2),
        _ => None,
    };
    keys.iter()
        .filter(|k| k.layer == 0)
        .filter_map(|k| rank(k.code).map(|r| (r, k.row, k.col)))
        .min()
        .map(|(_, r, c)| (r, c))
}

/// A character as the keymap and the statistics know it: capitals by their letter.
pub fn lower(c: char) -> char {
    c.to_lowercase().next().unwrap_or(c)
}

/// Words made into short sentences: a capital to start (when the keymap has a
/// Shift), a comma now and then, a full stop to end, each only if the keymap types it.
pub fn punctuate(words: &str, caps: bool, period: bool, comma: bool, rng: &mut Rng) -> String {
    let mut out = String::new();
    let mut left = 0;
    let list: Vec<&str> = words.split(' ').filter(|w| !w.is_empty()).collect();
    for (i, w) in list.iter().enumerate() {
        let starts = left == 0;
        if starts {
            left = 3 + rng.below(4);
        }
        if !out.is_empty() {
            out.push(' ');
        }
        let mut chars = w.chars();
        match chars.next() {
            Some(first) if starts && caps => {
                out.extend(first.to_uppercase());
                out.push_str(chars.as_str());
            }
            _ => out.push_str(w),
        }
        left -= 1;
        let last = i + 1 == list.len();
        if (left == 0 || last) && period {
            out.push('.');
            left = 0;
        } else if comma && left > 1 && rng.below(6) == 0 {
            out.push(',');
        }
    }
    out
}

/// The language's letters this keymap can type, most frequent first.
pub fn alphabet(lang: Lang, places: &BTreeMap<char, Place>) -> Vec<char> {
    lang.frequency().chars().filter(|c| places.contains_key(c)).collect()
}

/// Running averages of one letter.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct KeyStat {
    pub ms: f32,
    pub errors: f32,
    pub samples: u32,
}

impl KeyStat {
    fn hit(&mut self, ms: Option<f32>) {
        if let Some(ms) = ms {
            self.ms = if self.samples == 0 { ms } else { self.ms + AVERAGE_WEIGHT * (ms - self.ms) };
            self.samples += 1;
        }
        self.errors *= 1.0 - AVERAGE_WEIGHT;
    }

    fn miss(&mut self) {
        self.errors += AVERAGE_WEIGHT * (1.0 - self.errors);
    }

    pub fn wpm(&self) -> Option<f32> {
        (self.samples > 0 && self.ms > 0.0).then(|| 60000.0 / (self.ms * CHARS_PER_WORD))
    }

    /// Speed against the target, 1.0 when reached.
    pub fn confidence(&self, target: u32) -> f32 {
        self.wpm().map_or(0.0, |w| w / target.max(1) as f32)
    }

    pub fn known(&self, target: u32) -> bool {
        self.samples >= MIN_SAMPLES && self.confidence(target) >= 1.0 && self.errors <= MAX_ERROR_RATE
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct LangProgress {
    pub unlocked: usize,
    pub keys: BTreeMap<char, KeyStat>,
    /// Letter pairs, "th", timed on their second key.
    #[serde(default)]
    pub pairs: BTreeMap<String, KeyStat>,
    /// "tr": "r" typed where "t" was wanted, and how many times.
    #[serde(default)]
    pub slips: BTreeMap<String, u32>,
    pub lines: u32,
    pub best_wpm: f32,
    /// Speed of the latest lines, oldest first.
    #[serde(default)]
    pub history: Vec<f32>,
}

impl LangProgress {
    fn remember(&mut self, wpm: f32) {
        self.history.push(wpm);
        if self.history.len() > HISTORY_LEN {
            self.history.remove(0);
        }
    }
}

impl LangProgress {
    pub fn letters<'a>(&self, alphabet: &'a [char]) -> &'a [char] {
        let n = self.unlocked.max(START_LETTERS).min(alphabet.len());
        &alphabet[..n]
    }

    pub fn stat(&self, c: char) -> KeyStat {
        self.keys.get(&c).copied().unwrap_or_default()
    }

    /// The letter to give more room: one not known yet, the slowest first.
    pub fn focus(&self, letters: &[char], target: u32) -> Option<char> {
        letters.iter().copied().min_by(|&a, &b| {
            let key = |c: char| (self.stat(c).known(target), self.stat(c).confidence(target));
            let (ka, kb) = (key(a), key(b));
            ka.0.cmp(&kb.0).then(ka.1.total_cmp(&kb.1))
        })
    }

    /// Once every letter in play is known, the next one comes in.
    fn after_line(&mut self, alphabet: &[char], target: u32) -> Option<char> {
        let letters = self.letters(alphabet);
        let all_known = letters.iter().all(|&c| self.stat(c).known(target));
        if all_known && letters.len() < alphabet.len() {
            self.unlocked = letters.len() + 1;
            return alphabet.get(letters.len()).copied();
        }
        None
    }
}

/// What is kept between sessions, inside the app's settings file.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Save {
    pub lang: String,
    pub target_wpm: u32,
    pub show_keyboard: bool,
    #[serde(default = "default_mode")]
    pub mode: String,
    /// Each key outlined in the colour of its finger.
    #[serde(default)]
    pub show_fingers: bool,
    /// Blank keycaps, to type without reading them.
    #[serde(default)]
    pub hide_labels: bool,
    #[serde(default)]
    pub langs: BTreeMap<String, LangProgress>,
    #[serde(default)]
    pub game: Game,
    /// Capitals and punctuation in Words and Sprint.
    #[serde(default)]
    pub capitals: bool,
}

fn default_mode() -> String {
    Mode::Letters.code().to_string()
}

impl Default for Save {
    fn default() -> Self {
        Save {
            lang: "en".into(),
            target_wpm: DEFAULT_TARGET_WPM,
            show_keyboard: true,
            mode: default_mode(),
            show_fingers: false,
            hide_labels: false,
            langs: BTreeMap::new(),
            game: Game::default(),
            capitals: false,
        }
    }
}

/// Small deterministic generator: lines don't need more.
#[derive(Clone, Debug)]
pub struct Rng(u64);

impl Rng {
    pub fn new(seed: u64) -> Rng {
        Rng(seed | 1)
    }

    fn next(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        x
    }

    fn below(&mut self, n: usize) -> usize {
        (self.next() % n.max(1) as u64) as usize
    }

    fn weighted(&mut self, weights: &[u32]) -> usize {
        let total: u32 = weights.iter().sum();
        let mut pick = self.below(total.max(1) as usize) as u32;
        for (i, &w) in weights.iter().enumerate() {
            if pick < w {
                return i;
            }
            pick -= w;
        }
        0
    }
}

/// How often a letter follows another (or starts a word) in the language's words.
fn pair_counts(words: &[&str]) -> BTreeMap<(Option<char>, char), u32> {
    let mut out = BTreeMap::new();
    for w in words {
        let mut prev = None;
        for c in w.chars() {
            *out.entry((prev, c)).or_insert(0) += 1;
            prev = Some(c);
        }
    }
    out
}

/// A made-up word of the letters in play, shaped like the language's words.
fn pseudo_word(
    pairs: &BTreeMap<(Option<char>, char), u32>,
    letters: &[char],
    must: Option<char>,
    rng: &mut Rng,
) -> String {
    let mut word = String::new();
    for _ in 0..12 {
        let len = 3 + rng.below(4);
        word.clear();
        let mut prev = None;
        for _ in 0..len {
            let weights: Vec<u32> = letters
                .iter()
                .map(|&c| 1 + 4 * pairs.get(&(prev, c)).copied().unwrap_or(0))
                .collect();
            let c = letters[rng.weighted(&weights)];
            word.push(c);
            prev = Some(c);
        }
        if must.is_none_or(|m| word.contains(m)) {
            return word;
        }
    }
    if let Some(m) = must {
        let at = word.char_indices().nth(word.chars().count() / 2).map_or(0, |(i, _)| i);
        word.insert(at, m);
    }
    word
}

/// One practice line: real words that fit the letters in play, made-up ones while
/// too few fit, and the focus letter in most words. With `real_only`, made-up words
/// only fill in when too few real ones fit.
pub fn line(lang: Lang, letters: &[char], focus: Option<char>, real_only: bool, rng: &mut Rng) -> String {
    let words: Vec<&str> = lang.words().split_whitespace().collect();
    let real: Vec<&str> = words.iter().copied().filter(|w| w.chars().all(|c| letters.contains(&c))).collect();
    let pairs = pair_counts(&words);
    let mut out = String::new();
    while out.chars().count() < LINE_CHARS {
        let must = focus.filter(|_| rng.below(10) < 6);
        let enough = real.len() >= MIN_REAL_WORDS;
        let word = if enough && (real_only || rng.below(2) == 0) {
            let mut pick = real[rng.below(real.len())];
            for _ in 0..8 {
                if must.is_none_or(|m| pick.contains(m)) {
                    break;
                }
                pick = real[rng.below(real.len())];
            }
            pick.to_string()
        } else {
            pseudo_word(&pairs, letters, must, rng)
        };
        if !out.is_empty() {
            out.push(' ');
        }
        out.push_str(&word);
    }
    out
}

pub struct LineResult {
    pub wpm: f32,
    pub accuracy: f32,
    pub new_letter: Option<char>,
    pub reward: Reward,
    /// When it was finished, in the app's seconds: the stars come in from there.
    pub at: f64,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Typed {
    Right,
    Wrong,
    LineDone,
}

/// One person practising: the line on screen and how it goes.
pub struct Session {
    pub save: Save,
    pub alphabet: Vec<char>,
    pub symbols: Vec<char>,
    /// Letter pairs typed by one finger on this keymap.
    pub same_finger: std::collections::BTreeSet<String>,
    /// The keymap has a key that gives capitals.
    pub has_shift: bool,
    pub text: Vec<char>,
    pub pos: usize,
    missed_here: bool,
    last_at: Option<f64>,
    started_at: Option<f64>,
    hits: u32,
    misses: u32,
    pub last: Option<LineResult>,
    pub wrong_until: f64,
    /// The key typed by mistake, while it is shown.
    pub wrong_char: Option<char>,
    /// When the running sprint began, and what it counted so far.
    pub sprint_start: Option<f64>,
    sprint_hits: u32,
    sprint_misses: u32,
    /// Keys in a row without a mistake, across lines.
    pub combo: u32,
    line_best_combo: u32,
    /// Days since 1970, set by the caller: what counts for the streak.
    pub today: u64,
    rng: Rng,
}

impl Session {
    pub fn new(save: Save, seed: u64) -> Session {
        Session {
            save,
            alphabet: Vec::new(),
            symbols: Vec::new(),
            same_finger: Default::default(),
            has_shift: false,
            text: Vec::new(),
            pos: 0,
            missed_here: false,
            last_at: None,
            started_at: None,
            hits: 0,
            misses: 0,
            last: None,
            wrong_until: 0.0,
            wrong_char: None,
            sprint_start: None,
            sprint_hits: 0,
            sprint_misses: 0,
            combo: 0,
            line_best_combo: 0,
            today: 0,
            rng: Rng::new(seed),
        }
    }

    pub fn lang(&self) -> Lang {
        Lang::from_code(&self.save.lang)
    }

    pub fn mode(&self) -> Mode {
        Mode::from_code(&self.save.mode)
    }

    /// Signs don't belong to a language: they have their own record.
    fn progress_key(&self) -> String {
        match self.mode() {
            Mode::Symbols => Mode::Symbols.code().to_string(),
            Mode::Sprint => format!("sprint-{}", self.lang().code()),
            _ => self.lang().code().to_string(),
        }
    }

    pub fn progress(&self) -> LangProgress {
        self.save.langs.get(&self.progress_key()).cloned().unwrap_or_default()
    }

    fn progress_mut(&mut self) -> &mut LangProgress {
        let key = self.progress_key();
        self.save.langs.entry(key).or_default()
    }

    pub fn letters(&self) -> Vec<char> {
        self.progress().letters(&self.alphabet).to_vec()
    }

    /// The characters this mode trains and times.
    pub fn pool(&self) -> Vec<char> {
        match self.mode() {
            Mode::Letters => self.letters(),
            Mode::Words | Mode::Sprint | Mode::Bigrams => self.alphabet.clone(),
            Mode::Symbols => self.symbols.clone(),
        }
    }

    pub fn focus(&self) -> Option<char> {
        self.progress().focus(&self.pool(), self.save.target_wpm)
    }

    /// Enough to make a line in this mode.
    pub fn can_practise(&self) -> bool {
        match self.mode() {
            Mode::Symbols => self.symbols.len() >= 2,
            _ => self.alphabet.len() >= START_LETTERS,
        }
    }

    /// The keymap or the language changed what can be typed: start a fresh line.
    pub fn set_alphabet(&mut self, alphabet: Vec<char>, symbols: Vec<char>) {
        if alphabet != self.alphabet || symbols != self.symbols || self.text.is_empty() {
            self.alphabet = alphabet;
            self.symbols = symbols;
            self.new_line();
        }
    }

    pub fn set_mode(&mut self, mode: Mode) {
        self.save.mode = mode.code().to_string();
        self.last = None;
        self.sprint_start = None;
        self.sprint_hits = 0;
        self.sprint_misses = 0;
        self.new_line();
    }

    pub fn set_lang(&mut self, lang: Lang) {
        self.save.lang = lang.code().to_string();
        self.alphabet.clear();
        self.text.clear();
    }

    pub fn new_line(&mut self) {
        let pool = self.pool();
        let focus = self.focus();
        let text = if !self.can_practise() || pool.is_empty() {
            String::new()
        } else if self.mode() == Mode::Symbols {
            symbol_line(&pool, focus, &mut self.rng)
        } else if self.mode() == Mode::Bigrams {
            let weak = self.weak_pairs();
            bigram_line(self.lang(), &pool, &weak, &mut self.rng)
        } else {
            let words = line(self.lang(), &pool, focus, self.mode() != Mode::Letters, &mut self.rng);
            if self.save.capitals && matches!(self.mode(), Mode::Words | Mode::Sprint) {
                let (period, comma) = (self.symbols.contains(&'.'), self.symbols.contains(&','));
                punctuate(&words, self.has_shift, period, comma, &mut self.rng)
            } else {
                words
            }
        };
        self.text = text.chars().collect();
        self.pos = 0;
        self.missed_here = false;
        self.last_at = None;
        self.started_at = None;
        self.hits = 0;
        self.misses = 0;
        self.line_best_combo = self.combo;
    }

    pub fn reset_progress(&mut self) {
        self.progress_mut().clone_from(&LangProgress::default());
        self.last = None;
        self.new_line();
    }

    /// Speed so far on this line, from its first key.
    pub fn live_wpm(&self, now: f64) -> Option<f32> {
        let minutes = (now - self.started_at?) as f32 / 60.0;
        (self.pos > 1 && minutes > 0.0).then(|| self.pos as f32 / CHARS_PER_WORD / minutes)
    }

    pub fn expected(&self) -> Option<char> {
        self.text.get(self.pos).copied()
    }

    /// One character typed at `now` seconds. A wrong one counts once against the
    /// letter wanted and doesn't move on.
    pub fn type_char(&mut self, c: char, now: f64) -> Typed {
        let Some(want) = self.expected() else { return Typed::Wrong };
        if self.mode() == Mode::Sprint {
            if self.sprint_left(now) == Some(0.0) {
                return Typed::Wrong;
            }
            self.sprint_start.get_or_insert(now);
            if c == want { self.sprint_hits += 1 } else if !self.missed_here { self.sprint_misses += 1 }
        }
        self.started_at.get_or_insert(now);
        let key = lower(want);
        let tracked = self.pool().contains(&key);
        if c != want {
            if !self.missed_here {
                self.misses += 1;
                if tracked {
                    self.progress_mut().keys.entry(key).or_default().miss();
                }
                *self.progress_mut().slips.entry([want, c].iter().collect()).or_default() += 1;
                if let Some(pair) = self.pair_here() {
                    self.progress_mut().pairs.entry(pair).or_default().miss();
                }
                self.missed_here = true;
            }
            self.wrong_until = now + 0.3;
            self.wrong_char = Some(c);
            self.combo = 0;
            return Typed::Wrong;
        }
        // A key typed after a mistake also took the time of the mistake.
        let gap = self
            .last_at
            .map(|t| ((now - t) * 1000.0) as f32)
            .filter(|&ms| ms <= PAUSE_MS && !self.missed_here);
        if tracked {
            self.progress_mut().keys.entry(key).or_default().hit(gap);
        }
        if let Some(pair) = self.pair_here() {
            self.progress_mut().pairs.entry(pair).or_default().hit(gap);
        }
        self.hits += 1;
        self.combo += 1;
        self.line_best_combo = self.line_best_combo.max(self.combo);
        self.last_at = Some(now);
        self.missed_here = false;
        self.pos += 1;
        if self.pos < self.text.len() {
            return Typed::Right;
        }
        self.finish_line(now);
        Typed::LineDone
    }

    /// The letter pair that ends at the cursor, when both are letters of the alphabet.
    fn pair_here(&self) -> Option<String> {
        let prev = lower(*self.text.get(self.pos.checked_sub(1)?)?);
        let cur = lower(*self.text.get(self.pos)?);
        (self.alphabet.contains(&prev) && self.alphabet.contains(&cur)).then(|| [prev, cur].iter().collect())
    }

    /// Pairs to train: the slowest measured ones not known yet, then the language's
    /// common pairs, those typed by one finger first.
    pub fn weak_pairs(&self) -> Vec<String> {
        let p = self.progress();
        let target = self.save.target_wpm;
        let fits = |pair: &str| pair.chars().all(|c| self.alphabet.contains(&c));
        let mut measured: Vec<(&String, KeyStat)> = p
            .pairs
            .iter()
            .filter(|(k, st)| fits(k) && st.samples >= PAIR_SAMPLES && !st.known(target))
            .map(|(k, st)| (k, *st))
            .collect();
        measured.sort_by(|a, b| a.1.confidence(target).total_cmp(&b.1.confidence(target)));
        let mut out: Vec<String> = measured.into_iter().take(WEAK_PAIRS).map(|(k, _)| k.clone()).collect();
        if out.len() < WEAK_PAIRS {
            let words: Vec<&str> = self.lang().words().split_whitespace().collect();
            let mut common: Vec<(String, u32)> = pair_counts(&words)
                .into_iter()
                .filter_map(|((a, b), n)| a.map(|a| ([a, b].iter().collect::<String>(), n)))
                .filter(|(k, _)| fits(k) && !p.pairs.get(k).is_some_and(|st| st.known(target)))
                .collect();
            common.sort_by_key(|(k, n)| (!self.same_finger.contains(k), std::cmp::Reverse(*n)));
            for (k, _) in common {
                if out.len() >= WEAK_PAIRS {
                    break;
                }
                if !out.contains(&k) {
                    out.push(k);
                }
            }
        }
        out
    }

    /// Seconds left in the running sprint.
    pub fn sprint_left(&self, now: f64) -> Option<f64> {
        self.sprint_start.map(|t| (SPRINT_SECONDS - (now - t)).max(0.0))
    }

    /// Ends the sprint when its time is up. True when it did: there is a result to keep.
    pub fn tick(&mut self, now: f64) -> bool {
        if self.mode() != Mode::Sprint || self.sprint_left(now) != Some(0.0) {
            return false;
        }
        let wpm = self.sprint_hits as f32 / CHARS_PER_WORD / (SPRINT_SECONDS / 60.0) as f32;
        let accuracy = self.sprint_hits as f32 / (self.sprint_hits + self.sprint_misses).max(1) as f32;
        let facts = LineFacts {
            chars: self.sprint_hits as usize,
            wpm,
            accuracy,
            target_wpm: self.save.target_wpm,
            best_combo: self.line_best_combo,
            day: self.today,
            all_letters: false,
            all_symbols: false,
            sprint: true,
        };
        let p = self.progress_mut();
        p.lines += 1;
        p.best_wpm = p.best_wpm.max(wpm);
        p.remember(wpm);
        let reward = self.save.game.finish_line(&facts);
        self.last = Some(LineResult { wpm, accuracy, new_letter: None, reward, at: now });
        self.sprint_start = None;
        self.sprint_hits = 0;
        self.sprint_misses = 0;
        self.new_line();
        true
    }

    fn finish_line(&mut self, now: f64) {
        // A sprint goes on from line to line; the clock ends it, not the line.
        if self.mode() == Mode::Sprint {
            self.new_line();
            return;
        }
        let minutes = (now - self.started_at.unwrap_or(now)) as f32 / 60.0;
        let wpm = if minutes > 0.0 { self.text.len() as f32 / CHARS_PER_WORD / minutes } else { 0.0 };
        let accuracy = self.hits as f32 / (self.hits + self.misses).max(1) as f32;
        let target = self.save.target_wpm;
        let alphabet = self.alphabet.clone();
        let letters_mode = self.mode() == Mode::Letters;
        let p = self.progress_mut();
        p.lines += 1;
        p.best_wpm = p.best_wpm.max(wpm);
        p.remember(wpm);
        let new_letter = if letters_mode { p.after_line(&alphabet, target) } else { None };
        let all_letters = letters_mode && !alphabet.is_empty() && p.letters(&alphabet).len() == alphabet.len();
        let symbols_mode = self.mode() == Mode::Symbols;
        let p = self.progress();
        let all_symbols = symbols_mode && self.symbols.iter().all(|&c| p.stat(c).known(target));
        let facts = LineFacts {
            chars: self.text.len(),
            wpm,
            accuracy,
            target_wpm: target,
            best_combo: self.line_best_combo,
            day: self.today,
            all_letters,
            all_symbols,
            sprint: false,
        };
        let reward = self.save.game.finish_line(&facts);
        self.last = Some(LineResult { wpm, accuracy, new_letter, reward, at: now });
        self.new_line();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(layer: usize, row: usize, col: usize, code: u16) -> Key {
        Key { layer, row, col, code, on_right_half: false }
    }

    #[test]
    fn a_key_types_the_character_of_its_label() {
        let q = KeyboardLayout::Qwerty;
        assert_eq!(char_of(0x08, &q), Some('e'));
        assert_eq!(char_of(0x2C, &q), Some(' '));
        // Layer-tap and mod-tap count by their tap.
        assert_eq!(char_of(0x4111, &q), Some('n'));
        assert_eq!(char_of(0x5108, &q), Some('e'));
        assert_eq!(char_of(0x0200, &q), None);
        assert_eq!(char_of(0x29, &q), None, "Escape types nothing");
    }

    #[test]
    fn the_computers_layout_decides_the_character() {
        let a = KeyboardLayout::Azerty;
        assert_eq!(char_of(0x14, &a), Some('a'));
        assert_eq!(char_of(0x1F, &a), Some('é'));
    }

    #[test]
    fn a_character_on_another_layer_names_the_key_to_hold() {
        let keys = [key(0, 0, 0, 0x08), key(0, 0, 1, 0x0200), key(1, 0, 3, 0x2F), key(2, 0, 4, 0x30)];
        let p = places(&keys, &KeyboardLayout::Qwerty);
        assert_eq!(p[&'e'], Place { layer: 0, row: 0, col: 0, hold: None });
        assert_eq!(p[&'['].hold, Some((0, 1)));
        assert!(!p.contains_key(&']'), "nothing holds layer 2");
    }

    #[test]
    fn a_plain_layer_hold_comes_before_one_that_adds_a_modifier() {
        // LM 2 GUI first in the matrix, MO 2 after it.
        let keys = [key(0, 0, 0, 0x7082), key(0, 0, 1, 0x0300), key(2, 1, 1, 0x2F)];
        assert_eq!(places(&keys, &KeyboardLayout::Qwerty)[&'['].hold, Some((0, 1)));
    }

    #[test]
    fn the_lowest_layer_wins() {
        let keys = [key(1, 0, 0, 0x08), key(0, 1, 1, 0x08), key(0, 0, 0, 0x0200)];
        assert_eq!(places(&keys, &KeyboardLayout::Qwerty)[&'e'].layer, 0);
    }

    #[test]
    fn the_alphabet_keeps_the_language_order_and_what_the_keymap_has() {
        let keys = [key(0, 0, 0, 0x04), key(0, 0, 1, 0x08), key(0, 0, 2, 0x17)];
        let p = places(&keys, &KeyboardLayout::Qwerty);
        assert_eq!(alphabet(Lang::English, &p), vec!['e', 't', 'a']);
    }

    #[test]
    fn a_line_only_uses_the_letters_in_play() {
        let letters = ['e', 't', 'a', 'o', 'i', 'n'];
        for seed in 1..20 {
            let mut rng = Rng::new(seed);
            let l = line(Lang::English, &letters, Some('o'), false, &mut rng);
            assert!(l.chars().count() >= LINE_CHARS);
            assert!(l.chars().all(|c| c == ' ' || letters.contains(&c)), "{l}");
            assert!(l.matches('o').count() >= 3, "the focus letter comes often: {l}");
        }
    }

    #[test]
    fn words_and_sprint_lines_are_only_real_words() {
        let letters: Vec<char> = Lang::French.frequency().chars().collect();
        let words: Vec<&str> = Lang::French.words().split_whitespace().collect();
        for seed in 1..10 {
            let mut rng = Rng::new(seed);
            let l = line(Lang::French, &letters, Some('e'), true, &mut rng);
            assert!(l.split(' ').all(|w| words.contains(&w)), "{l}");
        }
    }

    #[test]
    fn real_words_come_once_enough_fit() {
        let letters: Vec<char> = Lang::English.frequency().chars().take(12).collect();
        let mut rng = Rng::new(7);
        let l = line(Lang::English, &letters, None, false, &mut rng);
        let real = l.split(' ').filter(|w| Lang::English.words().split_whitespace().any(|r| r == *w)).count();
        assert!(real >= 2, "{l}");
    }

    fn session(text: &str) -> Session {
        let mut s = Session::new(Save::default(), 3);
        s.alphabet = "etaoin".chars().collect();
        s.text = text.chars().collect();
        s
    }

    #[test]
    fn a_wrong_key_counts_once_and_does_not_move_on() {
        let mut s = session("tea");
        assert_eq!(s.type_char('x', 0.0), Typed::Wrong);
        assert_eq!(s.type_char('y', 0.1), Typed::Wrong);
        assert_eq!(s.pos, 0);
        assert!(s.progress().stat('t').errors > 0.0);
        assert_eq!(s.type_char('t', 0.2), Typed::Right);
        assert_eq!(s.pos, 1);
    }

    #[test]
    fn a_finished_line_gives_its_speed_and_accuracy() {
        // 10 characters in 2 s: 2 words in 1/30 minute, 60 words per minute.
        let mut s = session("tea at neo");
        let mut t = 0.0;
        let mut done = Typed::Right;
        for (i, c) in "tea at neo".chars().enumerate() {
            if i == 3 {
                s.type_char('z', t);
            }
            done = s.type_char(c, t);
            t += 2.0 / 9.0;
        }
        assert_eq!(done, Typed::LineDone);
        let r = s.last.as_ref().unwrap();
        assert!((r.wpm - 60.0).abs() < 0.5, "{}", r.wpm);
        assert!((r.accuracy - 10.0 / 11.0).abs() < 1e-4);
        assert_eq!(s.progress().lines, 1);
    }

    #[test]
    fn a_shifted_sign_of_the_firmware_counts_on_qwerty_only() {
        assert_eq!(char_of(0x821E, &KeyboardLayout::Qwerty), Some('!'));
        assert_eq!(char_of(0x821E, &KeyboardLayout::Azerty), None, "no glyph it can vouch for");
        assert_eq!(char_of(0x8204, &KeyboardLayout::Qwerty), None, "Shift+A is not a sign");
    }

    #[test]
    fn the_symbols_are_what_the_layers_type_besides_letters() {
        let keys = [key(0, 0, 0, 0x08), key(0, 0, 1, 0x0200), key(0, 0, 2, 0x2C), key(1, 0, 3, 0x2F), key(1, 0, 4, 0x821E)];
        let p = places(&keys, &KeyboardLayout::Qwerty);
        assert_eq!(symbols(&p), vec!['!', '[']);
        assert_eq!(p[&'!'].hold, Some((0, 1)));
    }

    #[test]
    fn a_symbol_line_uses_only_the_signs_and_often_the_focus() {
        let signs = ['[', ']', '(', ')', '!'];
        let mut rng = Rng::new(5);
        let l = symbol_line(&signs, Some('!'), &mut rng);
        assert!(l.chars().count() >= LINE_CHARS);
        assert!(l.chars().all(|c| c == ' ' || signs.contains(&c)), "{l}");
        assert!(l.matches('!').count() >= 5, "{l}");
    }

    #[test]
    fn each_mode_trains_its_own_characters_and_only_letters_unlock() {
        let mut s = Session::new(Save::default(), 9);
        let alphabet: Vec<char> = "etaoinshr".chars().collect();
        s.set_alphabet(alphabet.clone(), vec!['[', ']']);
        assert_eq!(s.pool().len(), START_LETTERS);
        s.set_mode(Mode::Words);
        assert_eq!(s.pool(), alphabet);
        s.set_mode(Mode::Symbols);
        assert!(s.text.iter().all(|c| *c == ' ' || *c == '[' || *c == ']'));
        // Signs keep their own record, apart from the language's letters.
        let want = s.expected().unwrap();
        s.type_char('x', 0.0);
        assert!(s.save.langs["symbols"].keys[&want].errors > 0.0);
        assert!(!s.save.langs.contains_key("en"));
    }

    #[test]
    fn the_key_typed_by_mistake_is_kept_to_be_shown() {
        let mut s = session("tea");
        s.type_char('x', 1.0);
        assert_eq!(s.wrong_char, Some('x'));
        assert!(s.wrong_until > 1.0);
    }

    #[test]
    fn the_speed_shows_while_the_line_is_typed() {
        let mut s = session("tea at neo");
        assert_eq!(s.live_wpm(0.0), None);
        for (i, c) in "tea at".chars().enumerate() {
            s.type_char(c, i as f64 * 0.2);
        }
        // 6 characters in 1.2 s: 1.2 words in 1/50 minute.
        assert!((s.live_wpm(1.2).unwrap() - 60.0).abs() < 0.5);
    }

    #[test]
    fn an_older_settings_file_still_loads() {
        let old = serde_json::json!({"lang": "fr", "target_wpm": 40, "show_keyboard": true});
        let save: Save = serde_json::from_value(old).unwrap();
        assert!(!save.show_fingers && !save.hide_labels);
        assert_eq!(save.target_wpm, 40);
    }

    #[test]
    fn a_line_earns_stars_experience_and_keeps_the_combo() {
        let mut s = session("tea at neo");
        s.today = 42;
        s.type_char('x', 0.1);
        assert_eq!(s.combo, 0);
        let mut t = 0.2;
        for c in "tea at neo".chars() {
            s.type_char(c, t);
            t += 0.1;
        }
        let r = &s.last.as_ref().unwrap().reward;
        assert!(r.stars >= 3, "{}", r.stars);
        assert!(r.xp > 0);
        assert_eq!(s.combo, 10);
        assert_eq!(s.save.game.lines_on(42), 1);
        assert!(r.new_badges.contains(&"first_line"));
    }

    #[test]
    fn a_sprint_runs_across_lines_and_ends_on_the_clock() {
        let mut s = session("ta");
        s.save.mode = Mode::Sprint.code().into();
        s.alphabet = "etaoinshr".chars().collect();
        s.text = "ta".chars().collect();
        assert!(!s.tick(0.0), "the clock starts with the first key");
        s.type_char('t', 1.0);
        s.type_char('a', 1.2);
        assert!(s.last.is_none(), "a line doesn't end a sprint");
        assert!(!s.text.is_empty());
        assert_eq!(s.sprint_left(31.0), Some(30.0));
        assert!(!s.tick(30.0));
        assert!(s.tick(61.0));
        let r = s.last.as_ref().unwrap();
        // 2 keys in a minute.
        assert!((r.wpm - 0.4).abs() < 1e-4);
        assert_eq!(s.save.game.best_sprint, r.wpm);
        assert_eq!(s.progress().history.len(), 1);
        assert!(s.sprint_start.is_none());
    }

    #[test]
    fn the_chart_keeps_the_latest_speeds_only() {
        let mut p = LangProgress::default();
        for i in 0..HISTORY_LEN + 5 {
            p.remember(i as f32);
        }
        assert_eq!(p.history.len(), HISTORY_LEN);
        assert_eq!(p.history[0], 5.0);
    }

    #[test]
    fn each_pair_is_timed_on_its_second_key() {
        let mut s = session("tea");
        s.type_char('t', 0.0);
        s.type_char('x', 0.1);
        s.type_char('e', 0.5);
        s.type_char('a', 0.7);
        let p = s.progress();
        assert!(p.pairs["te"].errors > 0.0);
        assert_eq!(p.pairs["ea"].samples, 1);
        assert!((p.pairs["ea"].ms - 200.0).abs() < 1.0);
    }

    #[test]
    fn weak_pairs_put_the_slow_ones_then_the_same_finger_ones_first() {
        let mut s = session("x");
        s.alphabet = Lang::English.frequency().chars().collect();
        let slow = KeyStat { ms: 900.0, errors: 0.0, samples: PAIR_SAMPLES };
        s.progress_mut().pairs.insert("ou".into(), slow);
        s.same_finger.insert("ed".into());
        let weak = s.weak_pairs();
        assert_eq!(weak.len(), WEAK_PAIRS);
        assert_eq!(weak[0], "ou");
        assert_eq!(weak[1], "ed");
    }

    #[test]
    fn a_bigram_line_is_made_of_words_with_the_pairs() {
        let letters: Vec<char> = Lang::English.frequency().chars().collect();
        let pairs = vec!["th".to_string(), "qz".to_string()];
        let mut rng = Rng::new(4);
        let l = bigram_line(Lang::English, &letters, &pairs, &mut rng);
        assert!(l.chars().count() >= LINE_CHARS);
        assert!(l.split(' ').all(|w| w.contains("th") || w == "qzqz"), "{l}");
    }

    #[test]
    fn capitals_come_from_a_plain_shift_first_then_a_one_shot_then_a_home_row_mod() {
        let mt = key(0, 2, 1, 0x5204);
        let osm = key(0, 3, 0, 0x3002);
        let plain = key(0, 3, 5, 0xE1);
        assert_eq!(shift_key(&[mt, osm, plain]), Some((3, 5)));
        assert_eq!(shift_key(&[mt, osm]), Some((3, 0)));
        assert_eq!(shift_key(&[mt]), Some((2, 1)));
        assert_eq!(shift_key(&[key(0, 0, 0, 0x5104), key(1, 0, 0, 0xE1)]), None, "Ctrl, or off the base layer");
    }

    #[test]
    fn sentences_get_capitals_commas_and_full_stops_the_keymap_can_type() {
        let words = "the cat sat on a mat and then it ran far away from home";
        let mut rng = Rng::new(2);
        let t = punctuate(words, true, true, true, &mut rng);
        assert!(t.starts_with('T'), "{t}");
        assert!(t.ends_with('.'), "{t}");
        assert!(t.split(". ").all(|s| s.chars().next().is_some_and(char::is_uppercase)), "{t}");
        let plain = punctuate(words, false, false, false, &mut rng);
        assert_eq!(plain, words);
    }

    #[test]
    fn a_capital_counts_for_its_letter_and_a_slip_is_kept() {
        let mut s = session("Tea");
        s.type_char('r', 0.0);
        s.type_char('r', 0.1);
        s.type_char('t', 0.2);
        assert_eq!(s.pos, 0, "a small t is not a capital");
        s.type_char('T', 0.3);
        s.type_char('e', 0.5);
        let p = s.progress();
        assert_eq!(p.slips["Tr"], 1, "once per place");
        assert!(p.keys[&'t'].errors > 0.0);
        assert_eq!(p.pairs["te"].samples, 1);
    }

    #[test]
    fn a_pause_is_not_counted_as_the_time_of_a_key() {
        let mut s = session("ta");
        s.type_char('t', 0.0);
        s.type_char('a', 5.0);
        assert_eq!(s.progress().stat('a').samples, 0);
    }

    #[test]
    fn the_next_letter_comes_once_every_letter_in_play_is_known() {
        let alphabet: Vec<char> = "etaoinshr".chars().collect();
        let mut p = LangProgress::default();
        let fast = KeyStat { ms: 200.0, errors: 0.0, samples: MIN_SAMPLES };
        for &c in &alphabet[..5] {
            p.keys.insert(c, fast);
        }
        assert_eq!(p.after_line(&alphabet, 30), None, "'n' isn't known yet");
        assert_eq!(p.focus(p.letters(&alphabet), 30), Some('n'));
        p.keys.insert('n', fast);
        assert_eq!(p.after_line(&alphabet, 30), Some('s'));
        assert_eq!(p.letters(&alphabet).len(), 7);
    }

    #[test]
    fn a_known_letter_is_fast_enough_often_enough_and_rarely_missed() {
        // 30 wpm is 150 characters a minute: 400 ms a key.
        let at = |ms, errors, samples| KeyStat { ms, errors, samples }.known(30);
        assert!(at(400.0, 0.0, MIN_SAMPLES));
        assert!(!at(401.0, 0.0, MIN_SAMPLES));
        assert!(!at(300.0, 0.0, MIN_SAMPLES - 1));
        assert!(!at(300.0, 0.06, MIN_SAMPLES));
    }

    #[test]
    fn progress_survives_the_settings_file() {
        let mut save = Save::default();
        save.langs.entry("fr".into()).or_default().keys.insert('é', KeyStat { ms: 300.0, errors: 0.1, samples: 4 });
        let back: Save = serde_json::from_value(serde_json::to_value(&save).unwrap()).unwrap();
        assert_eq!(back.langs["fr"].keys[&'é'].samples, 4);
    }
}
