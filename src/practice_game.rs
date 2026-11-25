//! The game around the Practice tab: stars per line as on TypingClub, experience
//! and levels with the keyboard's own Tamagotchi rule, a combo of keys without a
//! mistake, a streak of days and badges.

use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

/// Experience a level asks before the next one: the same rule as the keyboard's
/// Tamagotchi (`level x 250 + 500`, read in the original at `0x26a99e`).
pub fn xp_for_level(level: u32) -> u32 {
    level * 250 + 500
}

/// Level reached with this much experience, and the experience into it.
pub fn level_of(xp: u32) -> (u32, u32) {
    let (mut level, mut left) = (0, xp);
    while left >= xp_for_level(level) {
        left -= xp_for_level(level);
        level += 1;
    }
    (level, left)
}

/// Lines a day to meet the daily goal.
pub const DAILY_GOAL: u32 = 10;
/// Most a combo adds to a line's experience: twice as much.
const COMBO_BONUS_CAP: u32 = 100;

/// Stars for one line, 1 to 5: finished; 90% right; 97% right; three quarters of
/// the target speed; the target speed.
pub fn stars(wpm: f32, accuracy: f32, target_wpm: u32) -> u8 {
    let target = target_wpm.max(1) as f32;
    1 + u8::from(accuracy >= 0.90)
        + u8::from(accuracy >= 0.97)
        + u8::from(wpm >= 0.75 * target)
        + u8::from(wpm >= target)
}

/// Experience for a line: each character counts once per star, and the longest
/// run without a mistake adds up to as much again.
pub fn line_xp(chars: usize, stars: u8, best_combo: u32) -> u32 {
    let base = chars as u32 * stars as u32;
    base + base * best_combo.min(COMBO_BONUS_CAP) / COMBO_BONUS_CAP
}

pub struct Badge {
    pub id: &'static str,
    pub name: &'static str,
    pub about: &'static str,
}

pub const BADGES: [Badge; 16] = [
    Badge { id: "first_line", name: "First line", about: "Finish a line" },
    Badge { id: "five_stars", name: "Five stars", about: "Five stars on a line" },
    Badge { id: "perfect", name: "Spotless", about: "A line without a single mistake" },
    Badge { id: "combo_50", name: "On a roll", about: "50 keys in a row without a mistake" },
    Badge { id: "combo_200", name: "Unstoppable", about: "200 keys in a row without a mistake" },
    Badge { id: "wpm_40", name: "40 wpm", about: "A line at 40 words per minute" },
    Badge { id: "wpm_60", name: "60 wpm", about: "A line at 60 words per minute" },
    Badge { id: "wpm_80", name: "80 wpm", about: "A line at 80 words per minute" },
    Badge { id: "all_letters", name: "Full alphabet", about: "Every letter of a language in play" },
    Badge { id: "symbols", name: "Layer master", about: "Every sign of the keymap known" },
    Badge { id: "goal", name: "Daily goal", about: "10 lines in one day" },
    Badge { id: "streak_3", name: "Three days", about: "Practise three days in a row" },
    Badge { id: "streak_7", name: "A week", about: "Practise seven days in a row" },
    Badge { id: "lines_100", name: "Hundred lines", about: "100 lines in all" },
    Badge { id: "level_10", name: "Level 10", about: "Reach level 10" },
    Badge { id: "sprint", name: "Sprinter", about: "Finish a 60-second sprint" },
];

pub fn badge(id: &str) -> Option<&'static Badge> {
    BADGES.iter().find(|b| b.id == id)
}

/// What the game keeps between sessions.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Game {
    pub xp: u32,
    pub best_combo: u32,
    pub total_lines: u32,
    pub streak: u32,
    /// Days since 1970 of the last line, in UTC.
    pub last_day: u64,
    pub today_lines: u32,
    pub badges: BTreeSet<String>,
    /// Best 60-second sprint, in words per minute.
    pub best_sprint: f32,
}

/// One line, as the game sees it.
pub struct LineFacts {
    pub chars: usize,
    pub wpm: f32,
    pub accuracy: f32,
    pub target_wpm: u32,
    pub best_combo: u32,
    pub day: u64,
    pub all_letters: bool,
    pub all_symbols: bool,
    /// A whole 60-second sprint rather than one line.
    pub sprint: bool,
}

/// What a line earned.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Reward {
    pub stars: u8,
    pub xp: u32,
    pub level_up: Option<u32>,
    pub new_badges: Vec<&'static str>,
}

impl Game {
    pub fn level(&self) -> (u32, u32) {
        level_of(self.xp)
    }

    /// Lines done today, as of `day`.
    pub fn lines_on(&self, day: u64) -> u32 {
        if self.last_day == day { self.today_lines } else { 0 }
    }

    /// The streak as of `day`: it lasts until a whole day goes by without a line.
    pub fn streak_on(&self, day: u64) -> u32 {
        if day <= self.last_day + 1 { self.streak } else { 0 }
    }

    pub fn finish_line(&mut self, f: &LineFacts) -> Reward {
        let stars = stars(f.wpm, f.accuracy, f.target_wpm);
        let xp = line_xp(f.chars, stars, f.best_combo);
        let level_before = self.level().0;
        self.xp += xp;
        self.best_combo = self.best_combo.max(f.best_combo);
        if f.sprint {
            self.best_sprint = self.best_sprint.max(f.wpm);
        }
        self.total_lines += 1;
        if self.last_day == f.day {
            self.today_lines += 1;
        } else {
            self.streak = if f.day == self.last_day + 1 { self.streak + 1 } else { 1 };
            self.today_lines = 1;
            self.last_day = f.day;
        }
        let level = self.level().0;
        let earned = [
            ("first_line", true),
            ("five_stars", stars == 5),
            ("perfect", f.accuracy >= 1.0),
            ("combo_50", self.best_combo >= 50),
            ("combo_200", self.best_combo >= 200),
            ("wpm_40", f.wpm >= 40.0),
            ("wpm_60", f.wpm >= 60.0),
            ("wpm_80", f.wpm >= 80.0),
            ("all_letters", f.all_letters),
            ("symbols", f.all_symbols),
            ("goal", self.today_lines >= DAILY_GOAL),
            ("streak_3", self.streak >= 3),
            ("streak_7", self.streak >= 7),
            ("lines_100", self.total_lines >= 100),
            ("level_10", level >= 10),
            ("sprint", f.sprint),
        ];
        let new_badges = earned
            .into_iter()
            .filter(|(id, got)| *got && self.badges.insert(id.to_string()))
            .map(|(id, _)| id)
            .collect();
        Reward { stars, xp, level_up: (level > level_before).then_some(level), new_badges }
    }
}

/// Today, in days since 1970 (UTC).
pub fn today() -> u64 {
    web_time::SystemTime::now()
        .duration_since(web_time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs() / 86_400)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn facts(wpm: f32, accuracy: f32, day: u64) -> LineFacts {
        LineFacts {
            chars: 48,
            wpm,
            accuracy,
            target_wpm: 30,
            best_combo: 0,
            day,
            all_letters: false,
            all_symbols: false,
            sprint: false,
        }
    }

    #[test]
    fn levels_follow_the_keyboards_tamagotchi() {
        assert_eq!(xp_for_level(0), 500);
        assert_eq!(xp_for_level(4), 1500);
        assert_eq!(level_of(0), (0, 0));
        assert_eq!(level_of(499), (0, 499));
        assert_eq!(level_of(500), (1, 0));
        assert_eq!(level_of(500 + 750 + 10), (2, 10));
    }

    #[test]
    fn stars_reward_accuracy_then_speed() {
        assert_eq!(stars(5.0, 0.5, 30), 1);
        assert_eq!(stars(5.0, 0.95, 30), 2);
        assert_eq!(stars(5.0, 0.98, 30), 3);
        assert_eq!(stars(23.0, 0.98, 30), 4);
        assert_eq!(stars(30.0, 0.98, 30), 5);
    }

    #[test]
    fn a_long_combo_doubles_a_lines_experience_at_most() {
        assert_eq!(line_xp(48, 5, 0), 240);
        assert_eq!(line_xp(48, 5, 50), 360);
        assert_eq!(line_xp(48, 5, 1000), 480);
    }

    #[test]
    fn a_streak_grows_day_after_day_and_breaks_after_a_day_off() {
        let mut g = Game::default();
        g.finish_line(&facts(20.0, 0.9, 100));
        g.finish_line(&facts(20.0, 0.9, 100));
        assert_eq!((g.streak, g.lines_on(100)), (1, 2));
        g.finish_line(&facts(20.0, 0.9, 101));
        g.finish_line(&facts(20.0, 0.9, 102));
        assert_eq!(g.streak, 3);
        assert!(g.badges.contains("streak_3"));
        assert_eq!(g.streak_on(103), 3, "still on until the day is over");
        assert_eq!(g.streak_on(104), 0);
        g.finish_line(&facts(20.0, 0.9, 105));
        assert_eq!(g.streak, 1);
        assert_eq!(g.lines_on(106), 0);
    }

    #[test]
    fn a_badge_is_earned_once() {
        let mut g = Game::default();
        let first = g.finish_line(&facts(65.0, 1.0, 1));
        assert!(first.new_badges.contains(&"first_line"));
        assert!(first.new_badges.contains(&"wpm_60"));
        assert!(first.new_badges.contains(&"perfect"));
        assert!(!first.new_badges.contains(&"wpm_80"));
        let again = g.finish_line(&facts(65.0, 1.0, 1));
        assert!(again.new_badges.is_empty());
        assert!(BADGES.iter().all(|b| badge(b.id).is_some()));
    }

    #[test]
    fn a_sprint_keeps_its_record_and_its_badge() {
        let mut g = Game::default();
        let r = g.finish_line(&LineFacts { sprint: true, ..facts(42.0, 0.97, 3) });
        assert!(r.new_badges.contains(&"sprint"));
        g.finish_line(&LineFacts { sprint: true, ..facts(35.0, 0.97, 3) });
        g.finish_line(&facts(70.0, 0.97, 3));
        assert_eq!(g.best_sprint, 42.0, "only sprints count for the record");
    }

    #[test]
    fn crossing_a_level_is_announced() {
        let mut g = Game { xp: 450, ..Default::default() };
        let r = g.finish_line(&facts(30.0, 1.0, 1));
        assert_eq!(r.level_up, Some(1));
    }

    #[test]
    fn an_older_settings_file_has_no_game_yet() {
        let g: Game = serde_json::from_value(serde_json::json!({"xp": 7})).unwrap();
        assert_eq!((g.xp, g.streak), (7, 0));
        let g: Game = serde_json::from_value(serde_json::json!({})).unwrap();
        assert_eq!(g.xp, 0);
    }
}
