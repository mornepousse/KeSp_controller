/// Remaps HID key names to their visual representation based on the user's
/// keyboard layout (language).  Ported from the C# `KeyConverter.LayoutOverrides`.

// Display implementation for the keyboard layout picker in settings.
impl std::fmt::Display for KeyboardLayout {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.name())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum KeyboardLayout {
    Qwerty,
    Azerty,
    Qwertz,
    Dvorak,
    Colemak,
    Bepo,
    QwertyEs,
    QwertyPt,
    QwertyIt,
    QwertyNordic,
    QwertyBr,
    QwertyTr,
    QwertyUk,
}

impl KeyboardLayout {
    /// All known layout variants.
    pub fn all() -> &'static [KeyboardLayout] {
        &[
            KeyboardLayout::Qwerty,
            KeyboardLayout::Azerty,
            KeyboardLayout::Qwertz,
            KeyboardLayout::Dvorak,
            KeyboardLayout::Colemak,
            KeyboardLayout::Bepo,
            KeyboardLayout::QwertyEs,
            KeyboardLayout::QwertyPt,
            KeyboardLayout::QwertyIt,
            KeyboardLayout::QwertyNordic,
            KeyboardLayout::QwertyBr,
            KeyboardLayout::QwertyTr,
            KeyboardLayout::QwertyUk,
        ]
    }

    /// Human-readable display name.
    pub fn name(&self) -> &str {
        match self {
            KeyboardLayout::Qwerty => "QWERTY",
            KeyboardLayout::Azerty => "AZERTY",
            KeyboardLayout::Qwertz => "QWERTZ",
            KeyboardLayout::Dvorak => "DVORAK",
            KeyboardLayout::Colemak => "COLEMAK",
            KeyboardLayout::Bepo => "BEPO",
            KeyboardLayout::QwertyEs => "QWERTY_ES",
            KeyboardLayout::QwertyPt => "QWERTY_PT",
            KeyboardLayout::QwertyIt => "QWERTY_IT",
            KeyboardLayout::QwertyNordic => "QWERTY_NORDIC",
            KeyboardLayout::QwertyBr => "QWERTY_BR",
            KeyboardLayout::QwertyTr => "QWERTY_TR",
            KeyboardLayout::QwertyUk => "QWERTY_UK",
        }
    }

    /// Parse a layout name (case-insensitive). Falls back to `Qwerty`.
    pub fn from_name(s: &str) -> Self {
        match s.to_ascii_uppercase().as_str() {
            "QWERTY" => KeyboardLayout::Qwerty,
            "AZERTY" => KeyboardLayout::Azerty,
            "QWERTZ" => KeyboardLayout::Qwertz,
            "DVORAK" => KeyboardLayout::Dvorak,
            "COLEMAK" => KeyboardLayout::Colemak,
            "BEPO" | "BÉPO" => KeyboardLayout::Bepo,
            "QWERTY_ES" => KeyboardLayout::QwertyEs,
            "QWERTY_PT" => KeyboardLayout::QwertyPt,
            "QWERTY_IT" => KeyboardLayout::QwertyIt,
            "QWERTY_NORDIC" => KeyboardLayout::QwertyNordic,
            "QWERTY_BR" => KeyboardLayout::QwertyBr,
            "QWERTY_TR" => KeyboardLayout::QwertyTr,
            "QWERTY_UK" => KeyboardLayout::QwertyUk,
            _ => KeyboardLayout::Qwerty,
        }
    }
}

/// Given a `layout` and an HID key name (e.g. `"A"`, `"COMMA"`, `"SEMICOLON"`),
/// returns the visual label for that key on the given layout, or `None` when no
/// override exists (meaning the default / QWERTY label applies).
///
/// The lookup is **case-insensitive** on `hid_name`.
pub fn remap_key_label(layout: &KeyboardLayout, hid_name: &str) -> Option<&'static str> {
    // Normalise to uppercase for matching.
    let key = hid_name.to_ascii_uppercase();
    let key = key.as_str();

    match layout {
        // QWERTY has no overrides — it *is* the reference layout.
        KeyboardLayout::Qwerty => None,

        KeyboardLayout::Azerty => match key {
            "COMMA" | "COMM" | "COMA" => Some(";"),
            "SEMICOLON" | "SCOLON" | "SCLN" => Some(","),
            "PERIOD" | "DOT" => Some(":"),
            "SLASH" | "SLSH" | "/" => Some("!"),
            "M" => Some(","),
            "W" => Some("Z"),
            "Z" => Some("W"),
            "Q" => Some("A"),
            "A" => Some("Q"),
            "," => Some(";"),
            "." => Some(":"),
            ";" => Some("M"),
            "-" | "MINUS" | "MIN" => Some(")"),
            "BRACKET_LEFT" | "LBRCKT" | "LBRC" | "[" => Some("^"),
            "BRACKET_RIGHT" | "RBRCKT" | "RBRC" | "]" => Some("$"),
            "BACKSLASH" | "BSLSH" | "\\" => Some("<"),
            "APOSTROPHE" | "APO" | "QUOT" | "'" => Some("\u{00f9}"), // ù
            "1" => Some("& 1"),
            "2" => Some("\u{00e9} 2 ~"),    // é 2 ~
            "3" => Some("\" 3 #"),
            "4" => Some("' 4 }"),
            "5" => Some("( 5 ["),
            "6" => Some("- 6 |"),
            "7" => Some("\u{00e8} 7 `"),     // è 7 `
            "8" => Some("_ 8 \\"),
            "9" => Some("\u{00e7} 9 ^"),     // ç 9 ^
            "0" => Some("\u{00e0} 0 @"),     // à 0 @
            _ => None,
        },

        KeyboardLayout::Qwertz => match key {
            "Y" => Some("Z"),
            "Z" => Some("Y"),
            "MINUS" | "MIN" => Some("\u{00df}"),                  // ß
            "EQUAL" | "EQL" => Some("'"),
            "BRACKET_LEFT" | "LBRCKT" | "LBRC" => Some("\u{00fc}"), // ü
            "BRACKET_RIGHT" | "RBRCKT" | "RBRC" => Some("+"),
            "SEMICOLON" | "SCOLON" | "SCLN" => Some("\u{00f6}"),    // ö
            "APOSTROPHE" | "APO" | "QUOT" => Some("\u{00e4}"),      // ä
            "GRAVE" | "GRV" => Some("^"),
            "SLASH" | "SLSH" => Some("-"),
            "BACKSLASH" | "BSLSH" => Some("#"),
            _ => None,
        },

        KeyboardLayout::Dvorak => match key {
            "Q" => Some("'"),
            "W" => Some(","),
            "E" => Some("."),
            "R" => Some("P"),
            "T" => Some("Y"),
            "Y" => Some("F"),
            "U" => Some("G"),
            "I" => Some("C"),
            "O" => Some("R"),
            "P" => Some("L"),
            "A" => Some("A"),
            "S" => Some("O"),
            "D" => Some("E"),
            "F" => Some("U"),
            "G" => Some("I"),
            "H" => Some("D"),
            "J" => Some("H"),
            "K" => Some("T"),
            "L" => Some("N"),
            "SEMICOLON" | "SCOLON" | "SCLN" => Some("S"),
            "APOSTROPHE" | "APO" | "QUOT" => Some("-"),
            "Z" => Some(";"),
            "X" => Some("Q"),
            "C" => Some("J"),
            "V" => Some("K"),
            "B" => Some("X"),
            "N" => Some("B"),
            "M" => Some("M"),
            "COMMA" | "COMM" | "COMA" => Some("W"),
            "PERIOD" | "DOT" => Some("V"),
            "SLASH" | "SLSH" => Some("Z"),
            "MINUS" | "MIN" => Some("["),
            "EQUAL" | "EQL" => Some("]"),
            "BRACKET_LEFT" | "LBRCKT" | "LBRC" => Some("/"),
            "BRACKET_RIGHT" | "RBRCKT" | "RBRC" => Some("="),
            _ => None,
        },

        KeyboardLayout::Colemak => match key {
            "E" => Some("F"),
            "R" => Some("P"),
            "T" => Some("G"),
            "Y" => Some("J"),
            "U" => Some("L"),
            "I" => Some("U"),
            "O" => Some("Y"),
            "P" => Some(";"),
            "S" => Some("R"),
            "D" => Some("S"),
            "F" => Some("T"),
            "G" => Some("D"),
            "J" => Some("N"),
            "K" => Some("E"),
            "L" => Some("I"),
            "SEMICOLON" | "SCOLON" | "SCLN" => Some("O"),
            "N" => Some("K"),
            _ => None,
        },

        KeyboardLayout::Bepo => match key {
            "Q" => Some("B"),
            "W" => Some("E"),
            "E" => Some("P"),
            "R" => Some("O"),
            "T" => Some("E"),
            "Y" => Some("^"),
            "U" => Some("V"),
            "I" => Some("D"),
            "O" => Some("L"),
            "P" => Some("J"),
            "A" => Some("A"),
            "S" => Some("U"),
            "D" => Some("I"),
            "F" => Some("E"),
            "G" => Some(","),
            "H" => Some("C"),
            "J" => Some("T"),
            "K" => Some("S"),
            "L" => Some("R"),
            "SEMICOLON" | "SCOLON" | "SCLN" => Some("N"),
            "Z" => Some("A"),
            "X" => Some("Y"),
            "C" => Some("X"),
            "V" => Some("."),
            "B" => Some("K"),
            "N" => Some("'"),
            "M" => Some("Q"),
            "COMMA" | "COMM" | "COMA" => Some("G"),
            "PERIOD" | "DOT" => Some("H"),
            "SLASH" | "SLSH" => Some("F"),
            "1" => Some("\""),
            "2" => Some("<"),
            "3" => Some(">"),
            "4" => Some("("),
            "5" => Some(")"),
            "6" => Some("@"),
            "7" => Some("+"),
            "8" => Some("-"),
            "9" => Some("/"),
            "0" => Some("*"),
            _ => None,
        },

        KeyboardLayout::QwertyEs => match key {
            "MINUS" | "MIN" => Some("'"),
            "EQUAL" | "EQL" => Some("\u{00a1}"),                     // ¡
            "BRACKET_LEFT" | "LBRCKT" | "LBRC" => Some("`"),
            "BRACKET_RIGHT" | "RBRCKT" | "RBRC" => Some("+"),
            "SEMICOLON" | "SCOLON" | "SCLN" => Some("\u{00f1}"),    // ñ
            "APOSTROPHE" | "APO" | "QUOT" => Some("'"),
            "GRAVE" | "GRV" => Some("\u{00ba}"),                     // º
            "SLASH" | "SLSH" => Some("-"),
            "BACKSLASH" | "BSLSH" => Some("\u{00e7}"),               // ç
            _ => None,
        },

        KeyboardLayout::QwertyPt => match key {
            "MINUS" | "MIN" => Some("'"),
            "EQUAL" | "EQL" => Some("\u{00ab}"),                     // «
            "BRACKET_LEFT" | "LBRCKT" | "LBRC" => Some("+"),
            "BRACKET_RIGHT" | "RBRCKT" | "RBRC" => Some("'"),
            "SEMICOLON" | "SCOLON" | "SCLN" => Some("\u{00e7}"),    // ç
            "APOSTROPHE" | "APO" | "QUOT" => Some("\u{00ba}"),      // º
            "GRAVE" | "GRV" => Some("\\"),
            "SLASH" | "SLSH" => Some("-"),
            "BACKSLASH" | "BSLSH" => Some("~"),
            _ => None,
        },

        KeyboardLayout::QwertyIt => match key {
            "MINUS" | "MIN" => Some("'"),
            "EQUAL" | "EQL" => Some("\u{00ec}"),                     // ì
            "BRACKET_LEFT" | "LBRCKT" | "LBRC" => Some("\u{00e8}"), // è
            "BRACKET_RIGHT" | "RBRCKT" | "RBRC" => Some("+"),
            "SEMICOLON" | "SCOLON" | "SCLN" => Some("\u{00f2}"),    // ò
            "APOSTROPHE" | "APO" | "QUOT" => Some("\u{00e0}"),      // à
            "GRAVE" | "GRV" => Some("\\"),
            "SLASH" | "SLSH" => Some("-"),
            "BACKSLASH" | "BSLSH" => Some("\u{00f9}"),              // ù
            _ => None,
        },

        KeyboardLayout::QwertyNordic => match key {
            "MINUS" | "MIN" => Some("+"),
            "EQUAL" | "EQL" => Some("'"),
            "BRACKET_LEFT" | "LBRCKT" | "LBRC" => Some("\u{00e5}"), // å
            "BRACKET_RIGHT" | "RBRCKT" | "RBRC" => Some("\u{00a8}"), // ¨
            "SEMICOLON" | "SCOLON" | "SCLN" => Some("\u{00f6}"),    // ö
            "APOSTROPHE" | "APO" | "QUOT" => Some("\u{00e4}"),      // ä
            "GRAVE" | "GRV" => Some("\u{00a7}"),                     // §
            "SLASH" | "SLSH" => Some("-"),
            "BACKSLASH" | "BSLSH" => Some("'"),
            _ => None,
        },

        KeyboardLayout::QwertyBr => match key {
            "MINUS" | "MIN" => Some("-"),
            "EQUAL" | "EQL" => Some("="),
            "BRACKET_LEFT" | "LBRCKT" | "LBRC" => Some("'"),
            "BRACKET_RIGHT" | "RBRCKT" | "RBRC" => Some("["),
            "SEMICOLON" | "SCOLON" | "SCLN" => Some("\u{00e7}"),    // ç
            "APOSTROPHE" | "APO" | "QUOT" => Some("~"),
            "GRAVE" | "GRV" => Some("'"),
            "SLASH" | "SLSH" => Some(";"),
            "BACKSLASH" | "BSLSH" => Some("]"),
            _ => None,
        },

        KeyboardLayout::QwertyTr => match key {
            "MINUS" | "MIN" => Some("*"),
            "EQUAL" | "EQL" => Some("-"),
            "BRACKET_LEFT" | "LBRCKT" | "LBRC" => Some("\u{011f}"), // ğ
            "BRACKET_RIGHT" | "RBRCKT" | "RBRC" => Some("\u{00fc}"), // ü
            "SEMICOLON" | "SCOLON" | "SCLN" => Some("\u{015f}"),    // ş
            "APOSTROPHE" | "APO" | "QUOT" => Some("i"),
            "GRAVE" | "GRV" => Some("\""),
            "SLASH" | "SLSH" => Some("."),
            "BACKSLASH" | "BSLSH" => Some(","),
            _ => None,
        },

        KeyboardLayout::QwertyUk => match key {
            "GRAVE" | "GRV" => Some("`"),
            "MINUS" | "MIN" => Some("-"),
            "EQUAL" | "EQL" => Some("="),
            "BACKSLASH" | "BSLSH" => Some("#"),
            "APOSTROPHE" | "APO" | "QUOT" => Some("'"),
            _ => None,
        },
    }
}
