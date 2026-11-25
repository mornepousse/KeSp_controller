# How KeSp Controller works

The numbers below are also in `mechanics.json`.

## Where the facts come from

1. **The previous version's source**, given by its author (`KeSp_controller/`). `src/protocol/` was
   kept as is: it has no UI in it and carries its tests. Everything else is new egui code.
2. **The firmware**, given by its author (`KeSp_firmware-gitlab/`): board shapes, factory keymaps,
   slot sizes, how empty keys behave. `tools/import_boards.py` turns the board files into `boards/*.json`.
3. **The compiled program**, as a check: the source stays the reference, and its author pointed out
   that it is complete. Ghidra analysed it in 1123 s. The tool's own link to Ghidra couldn't open the
   result (the program is saved as `target-db58af9e306a.0`, the link asks for the name without `.0`), so
   the functions were decompiled from that saved project in a read-only run.
   The link works now. **Addresses here are the file's; Ghidra shows the same code `0x100000` higher**, so
   `0x340acf` is `FUN_00440acf` there. Read through the link at the right address: `0x340acf`, `0x33871f`,
   and two reply functions listed below. Asked at the wrong address at first, it answered about unrelated
   window code. The key catalogue `0x3a4c2f` was read with the built-in decompiler.
   Decompiled and read: `0x340acf`, `0x338636`, `0x3386c9`, `0x33871f`, `0x3affa3`, `0x3551a9`,
   `0x355a07`, `0x26a99e`, `0x36cdc5`. Ghidra treats the text-formatting call as never returning, so it
   cuts the functions that use it short; those were read as machine code instead (the bare code
   addresses below). Everything checked matched the source, the one defect found is listed under
   "Corrected on the way".

| Rule | Function or code address |
| --- | --- |
| Frame starts `KS`, length in 2 bytes | `0x3546cb` |
| CRC-8 polynomial 0x31 when sending / reading | `0x3546fa` / `0x3b0152` |
| Reply: first `KR`, status at +3, length at +4, needs 7 + length bytes | function `0x3affa3` |
| 115200 baud, 300 ms port timeout | function `0x355a07` |
| 1500 ms reply wait, 30 ms settle | `0x354846`, `0x35483c` |
| Vendor id 0xCAFE, or one of 3 product names; `Dongle` in the name marks a dongle | function `0x3551a9` |
| `Sec Confirm` compare, exact code `0x3E00` | `0x344667` |
| Hand gap 75 px (1.5 u) | `0x344f1f` |
| Layer-Tap: `key | (layer & 0xF) << 8 | 0x4000`; keys `2C 28 29 2A 2B`, then `5..9` = index - 1, otherwise Space | functions `0x338636`, `0x3386c9`, table `0xac4de4` |
| Mod-Tap: modifier table, then key: `0..35` = index + 4, `36..51` from a table, otherwise `A`; unknown modifier = Shift | function `0x340acf`, tables `0xa88090`, `0xa927c0` |
| Macro delay: step `0xFF`, then `(ms & 0xFF) / 10` | function `0x33871f` |
| Macro sequence bytes: name cut at 255, `kc:mod` pairs read as hex, a bad value counts as 0 | function `0x3b04f9` |
| Tap dance list: 8 slots, 5 bytes per entry, an index of 8 or more is ignored | function `0x3b10e9` |
| Leader modifier: `1 << (index - 1)` | `0x341561` |
| Key catalogue: every category, code range and order, `None` put first (read with the built-in decompiler, same as `keys.rs`) | function `0x3a4c2f` |
| Flash offsets `0`, `0x20000`, `0x220000` | table `0xabe8fa` |
| OTA 4096-byte chunks, 95% of the bar, `OTA_OK` / `OTA_DONE` / `OTA_FAIL` | `0x358e9e`, `0x358fce`, `0x359195`, `0x3591b2`, `0x3591cf` |
| `OTA_READY` error message | `0x358c1e` |
| Pairing timeout message | `0x35fd9b` |
| Timers: 50 ms, 5 s, 5 s, 1 s | `0x381434`, `0x38157a`, `0x3816f7`, `0x381863` |
| Window 1000 x 700, minimum 600 x 450 | `0x1205b7`, `0x12057c` |
| XP needed: `level x 250 + 500` | function `0x26a99e` |
| Heat steps 0.8 / 0.6 / 0.4 / 0.2 / 0.05 and their colours | `0x279d46` to `0x27a0da` |
| `No KaSe device found` | function `0x268241` |
| `kase_settings.json` next to the program, or in `.` if its folder is unknown | function `0x36cdc5` |

## Protocol

- Request `KS` + command + length (2 bytes LE) + payload + CRC-8 of the payload.
- Reply `KR` + command + status + length (2 bytes LE) + payload + CRC-8. Polynomial 0x31, init 0.
- 115200 baud. Replies wait 1500 ms, twice that for commands relayed by the dongle.
- `ERR_UNKNOWN` means "this device doesn't do that", not a failure.

Connecting reads, in this order: ping, features, (dongle: pair list), version, layout, layer names,
keymap of layer 0. The layout comes before the keymap because it carries the row count.

## Keycodes

| Range | Meaning |
| --- | --- |
| `0x00`-`0xE7` | HID usage. `0` = empty |
| `0x0100`-`0x0A00` | MO layer (`layer + 1` in the high byte) |
| `0x0B00`-`0x1400` | TO layer |
| `0x1500`-`0x2800` | Macro |
| `0x3100`-`0x310F` | One-shot layer |
| `0x3E00` | Sec Confirm. Warn on this exact code on the right half, not on the range |
| `0x4LKK` | Layer-Tap |
| `0x5MKK` | Mod-Tap |
| `0x6I00` | Tap Dance |
| `0x8MKK` | Modifier + key in one press. `0x82xx` = shifted symbol |

## What the firmware says about layers

`K_NO` and `K_TRNS` are both 0. An empty key falls back **one step**, to the layer it was reached from
(`key_processor.c:451`, `keymaps[last_layer]`), not through every layer. MO sets `last_layer` to the
layer it left; TO sets it to the new layer, so an empty key there stays empty.

The editor shows the base layer's key, dimmed, on an empty key. *Estimated*: at rest it can't know the
route, and the base layer is the usual one.

## Corrected on the way

Each of these was a behaviour of the previous version, kept out of this one:

- **Mod-Tap with a right-hand modifier gave a wrong key.** The firmware keeps 4 bits for the modifier
  (`K_MT_MOD`) and only treats `0x5xxx` as Mod-Tap (`K_IS_MT`). The table in the old program
  (`0xa88090`) holds `5100 5200 5400 5800 5000 7000 5000 D000`: RCtrl and RAlt gave a Mod-Tap with no
  modifier at all, RShift and RGUI a code that isn't a Mod-Tap. The builder now offers Ctrl, Shift, Alt, GUI.
- **Hand and finger stats were wrong on a Niphargus.** The rule only knows the 13 columns of a KaSe and
  sends anything else to the left pinky, so column 13, the far right of a Niphargus, counted as left. It
  now follows the firmware for 4 x 14 and 4 x 7 boards: right column = 13 - keymap column, fingers read on
  the factory home row, last row = thumbs. A half alone still counts as the left hand: its stats don't
  know which half it is. The rule was read in the source; it wasn't located in the binary, where it is
  inlined.
- Tap dance, combo result, key override, leader and macro slots are **one byte** in the protocol. A
  layer or Mod-Tap code used to be cut silently and become another key. The selector now offers only
  what fits.
- A new combo or leader took "number of items" as its index, which overwrote an existing one after a
  deletion. It now takes the lowest free index.
- A reply to a layer read carried no layer number: switching quickly could file it under the wrong layer.
- A key write that failed was ignored. It now stays marked as unsent and the status bar says so.
- The Tamagotchi card could never appear, nothing queried its status when opening the tab.
- Tap dance slots could only be edited if already filled: empty ones are now reachable.
- A matrix event split over two reads was lost.
- A 500 ms macro delay overflowed the byte before dividing.
- Factory reset, pairing reset and flashing now ask for confirmation.

## New: the keyboard editor

- Models: KaSe V2, KaSe V1, Niphargus, Niphargus Right alone, each with its shape and factory keymap.
  Offline the chosen model is drawn with its factory keymap. Connected, the keyboard's own description
  wins and the model only comes from the name it reports (`KaSe V2 Debug` = V2, `KaSe Dongle` = Niphargus).
- Factory layer: refused when connected to a keyboard of unknown model, so another model's keys can't
  be written to it.
- Keymap tab: select, drag from the palette, drag a key onto another to swap (Ctrl: copy), right-click
  menu, arrows, Delete, Enter, undo/redo, copy/paste a key or a whole layer, type to assign, find.
- Without a keyboard, edits wait as unsent. Once connected they're sent only on request.
- Layout tab: drag keys, box-select, turn with a handle, grid, nudge, duplicate, mirror, typed values,
  duplicate matrix positions flagged in red. The protocol has no command to write a layout, so it
  leaves as a file; "Use on Keymap tab" only changes the drawing.

## The dongle, as it really answers

Asked on the real dongle (firmware v4.2.0-beta.3), read-only: `RF_PAIR_LIST` gives `paired_count` 0, and
yet `GET_LAYOUT_JSON` gives the Niphargus (4 x 14, 52 keys) and `KEYMAP_GET` the real keymap, because this
dongle runs the keymap engine itself (`CONFIG_KASE_DONGLE_FUSION`). The previous version read 0 as "no
keyboard", which greyed out Keymap, Advanced, Macros and Stats. A dongle now counts as reachable when it
answers with a keymap, and the pair list only says what it says.

### Who the radio link is really about

`RF_STATUS` was read as "left half, right half" (that is what `CDC_DONGLE_INTEGRATION.md`, the 27-byte form and
the field names still say). The firmware's `bin_cmd_rf_status` and `rf_slot.h` say otherwise: the dongle's two
slots are the keyboard, which both halves share, and the Conchodytes mouse. The real dongle answers 51 bytes,
flags `0x0D`, keyboard 7963 packets, mouse 0, while the right half's battery sample is under 2 s old. So
"Right half: not linked" was the missing mouse. The pair list's `mac_left` and `mac_right` are those same two
slots (`rf_slot.h` keeps the old key names so nothing has to be paired again).

The card shows the keyboard and the mouse, then each half. Since the firmware's `rf_half_seen` (uncommitted when
read, already on the real dongle), `RF_STATUS` is 69 bytes: bytes 51..58 give each half's age, 59..66 its frames,
67 and 68 the retransmission percentage it last reported, `0xFF` before its first STATUS. The real dongle gave
left 3521 ms, 525 frames, 0 %, and right 20605 ms, 356 frames, 0 %. A 51-byte reply has none of this, so the card
then falls back on the age of the battery sample (`dongle_cache_get_battery`).

A half's bars use the retry factor of `rf_signal_q255`, `255 x (100 - retries) / 100`, with the usual thresholds
200, 140, 80, 30. Its age factor is left out on purpose: a half at rest sends nothing (the firmware's protocol
document says a growing age is normal), so that factor would read 0 on a healthy half. Past 60 s without a frame,
the half reads "asleep or out of range" with no bars, because its last figure is old by then. It is not a radio
strength: the nRF24 has no such measure. Until this firmware, every STATUS sent `link_q = 0`, so the slot's signal
was age alone: `255 x (1500 - age) / 1500` gives 184 at 415 ms and 129 at 736 ms, the values the real dongle read.

The keyboard slot drops soon after the last frame, so it reads "idle" rather than a red "not linked" while a half
was heard recently. Flags bits 2 and 3 say whether each radio module is mounted, which tells a missing module from
a mouse out of range. A slot that never received anything reads "not seen", not as a failure, because its age only
counts from startup. With `paired_count` 0 the link runs on factory settings (`rf_pairing.c`).

A reply now ends its wait as soon as the frame is complete. It used to wait for a port timeout of silence
each time: 300 ms per key written, 17 s to paste a Niphargus layer.

## What the real dongle showed on the other tabs

- `KEYSTATS_BIN` answers 4 x 7 where the keymap is 4 x 14. The firmware counts only the matrix it scans
  (`key_stats[MATRIX_ROWS][MATRIX_COLS]`, `cdc_binary_cmds.c:406`): on a Niphargus, presses of the other
  half aren't counted. The Stats tab says so. Hands and fingers still read the keymap's width.
- `LIST_MACROS` gave two macros made of keys pressed together (`Copier`, `Coller`: `E0 06`, `E0 19`), and
  the previous version's parser skipped those keys: they showed empty, and saving then restoring a config
  rewrote them with no keys at all. They are now read, shown, saved and restored with `MACRO_ADD`
  (`[slot][name_len][name][6 keys]`, `cdc_binary_cmds.c:763`). Older config files still load.
- `TAMA_QUERY` answers 22 zero bytes: the pet was removed from the firmware, its commands stay for
  compatibility. Its cards only show when an answer isn't all zeros.
- No presses counted yet no longer lists every key as dead.
- `FEATURES` announces `LM` and `OSM`, and the firmware has `K_MK`, none of which the selector could
  build. Added: `K_MK` `0x8000 | nibble << 8 | key`, `K_OSM` `0x3000 | mods`, `K_LM`
  `0x7000 | mods << 4 | layer` (`key_definitions.h`), which `decode_keycode` didn't read either.

## The tabs, remixed

At their author's request the 8 tabs became 5, by what the person is trying to do:
Keymap, Layout, Behaviors (macros, tap dance, combos, overrides, leader, layer rules, one page each),
Typing (heat keyboard, headline numbers, fingers, rows, busiest keys) and Device (status and dongle,
backup, firmware, matrix test, reset). No feature was removed.

- Combos: keys are clicked on a keyboard in the page instead of opening a dialog, and a listed combo
  lights up its two keys.
- Tap dance: all 8 slots in one table.
- Typing: one hue for the keyboard's heat, from the keycap to orange, with a scale. The Keymap heatmap keeps
  the previous version's steps. With nothing counted, the tab says so instead of showing empty bars.

## Compared with the firmware's command handlers

Every list and write of `cdc_binary_cmds.c` was read against the app:

- Key overrides: the list skips empty slots and sends each one's slot. The previous version threw the slot
  away and used the position in the list: with a hole, Del erased the wrong override and a new one could
  overwrite an existing one. Slots are now kept.
- Tap dances: the firmware has 16 slots (`TAP_DANCE_MAX_SLOTS`), the previous version read 8: slots 8 to 15
  were invisible, and missing from a backup. Macros: 20 keys (`MAX_MACROS`), the selector offered 10.
- A macro's name keeps 15 bytes and it keeps 24 steps (`MAX_MACRO_NAME_LENGTH`, `MACRO_MAX_STEPS`), the
  rest being dropped without a word. The editor now holds names to that and says when steps don't fit.
- A combo or a leader sequence that sends "None" is dropped by the list, an override needs a trigger:
  those forms can't be sent that way any more. 16 slots each.
- A new macro takes the lowest free slot instead of the highest + 1.

Formats that already matched: tap dance, combo and leader lists, `TD_SET`, `COMBO_SET`, `LEADER_SET`,
`KO_SET`, `MACRO_ADD_SEQ`, `SETLAYER`, the tri-layer command.

## In a browser

`tools/build_web.sh` builds the same app for WebAssembly into `web/`, and `tools/serve_web.sh` serves it on
localhost. It draws with WebGL. It reaches the keyboard through Web Serial, which only Chrome and Edge have,
and only on https or localhost. Requests are the same as on the desktop; they run in one queue that takes
turns with the page (`backend_web.rs`), because a browser tab has no threads to block.

- The browser shows its own device list on Connect, filtered on vendor ids `0xCAFE` and `0x303A`. Afterwards
  a device already allowed connects by itself at start.
- Opening a file asks the browser, saving is a download.
- Firmware updates aren't offered: a failed one in a tab can leave a keyboard unusable.

Loaded and drawn in headless Chromium. Web Serial itself needs a person to click in the browser's list, so
talking to a real keyboard from a browser hasn't been tried.

## The keymap check

Everything read at connection: all layers, macros, tap dances. From that, the Check button reports:

- A layer with keys that nothing reaches, starting from the base layer through MO, TO, OSL, LT and LM. A
  Tri-Layer rule could still reach it: the firmware has no command to read that rule back.
- A TO with no way out. `apply_toggle_layer` (`key_processor.c`) sets `last_layer` to the new layer, so
  empty keys fall back onto it and an MO returns to it: only another TO key leaves.
- A macro or tap dance key with nothing behind it, once those lists are read.
- Sec Confirm on the right half (exact code), the internal `K_WPM` code, empty keys on the base layer.

A layer read that an edit overtook is read again rather than filed, so it can't bring back the old key.

## Read as machine code

Ghidra merges the reply parsers that follow the tap dance list into one wrong function, so they can't be
decompiled. Read as machine code instead, all matching the source: the combo list (`0x3b12d4`, a count then
6 bytes per entry), the leader list (`0x3b1500`, index, length, then the sequence), the macro list
(`0x3b179e`, at least 4 bytes before the name). The old key override parser wasn't located. It isn't used any
more: `parse_ko_entries` follows the firmware's `bin_cmd_ko_list`. The real dongle's combo, leader and override
lists are empty, so those three have never read real content in this version.

The dongle's replies, read the same way, also match `parsers.rs`: radio status at `0x3456ae` (27 bytes at least,
checked at `0x3455e5`; flags bit 0 left and bit 1 right), battery at `0x345309` (14 bytes at least, checked at
`0x345231`; two slots of 7 bytes), pair list (13 bytes at least, `0x3b1084`), pair start (3 bytes at least,
`0x35fa7c`), signal bars at `0x345f90` (200, 140, 80, 30). Ghidra's pseudocode for this block was no use: it stops
at the first call it wrongly treats as never returning.
The 60 s limit of an old battery reading is at `0x345bac`, for the right half at `0x345c37`, written as one
comparison: age + `0xffff159f` below `0xffff159e`, which is true above 60000 ms and false for the "never" value.
Ghidra has no function there either. `HALF_HEARD_MS` keeps that number.

## Tried on real hardware

On the real dongle (firmware v4.2.0-beta.3): connecting, and every read of the Keymap, Behaviors, Typing and
Device tabs. Changing a key on the real keyboard works, as tried by its owner.

## Not tested on real hardware

The serial code also runs against a fake firmware on a pseudo-terminal in the tests, and the window logic
against simulated mouse and keyboard. These haven't run for real in this version: saving a macro, combo, tap
dance, override or leader sequence, restoring a config (including macros with `MACRO_ADD`), OTA, flashing,
pairing, the matrix test and the resets.

## Practice tab

Touch typing on the keyboard's own keymap. What exists was looked at first: keybr.com's README says it starts
with a small set of the language's most frequent letters, adds more once the target speed is reached with the
current ones, and focuses lessons on the weakest keys; TypingClub grades lessons with stars and shows the hand
posture on every key; ngram-type drills bigrams and common words against speed and accuracy thresholds.

The tab follows keybr's method, but on this keymap: the letters are those it can type, read from each key's
label on the computer's layout (the first sign of an AZERTY "é 2"), a layer-tap or mod-tap counting by its tap.
A character on another layer is shown with the base-layer key that holds that layer (`checks::layer_target`); a
layer nothing holds is left out. The next key, its finger (the firmware's Niphargus rule, the KaSe one otherwise,
with thumbs only on the last row) and the key to hold are lit on the board.

- Start with the 6 most frequent letters of English (`etaoinshrdlcumwfgypbvkjxqz`) or French
  (`esaitnrulodcpmévqfbghjàxèyêzwçùkœ`) this keymap has.
- Words per minute count 5 characters a word, so 30 wpm is 400 ms a key (`12000 / wpm`).
- A letter is known after 10 presses, at or above the target speed, with at most 5% misses. Its speed and
  misses are running averages that weigh the newest press 0.1. A gap over 2 s is a pause, not a key's time,
  and a key typed after a mistake doesn't time either.
- After each line, if every letter in play is known, the next one comes in. The focus letter is the slowest one
  not known yet; it is in about 6 words out of 10.
- Lines are 48 characters: real words from a built-in list once 12 of them fit the letters in play, made-up
  words otherwise, letter by letter weighted by how often each letter follows the previous one in the list.
- A wrong key counts once against the letter wanted and doesn't move on; the key typed lights red for 0.3 s.
- Three modes: Letters (the keybr method above), Words (real words with every letter, no new letter), and
  Symbols (the keymap's signs and digits in groups of 2 to 4, the focus one in about 3 of 10, with their own
  record apart from any language). A shifted sign of the firmware (`0x82KK`) counts only where the layout can
  name it (`layout_remap::shifted_label`: "!" on QWERTY, "Shift+1" elsewhere, left out).
- A sign on another layer is shown with a plain hold (MO, LT, OSL) before a layer-mod key (LM): LM adds its
  modifier, so the sign would leave as a shortcut. On the real Niphargus, layer 2 is held by MO 2, not LM 2 GUI.
- Options: finger colours on every key (thumbs only on the last row), blank keycaps, live speed under the line,
  and each letter's speed, accuracy and presses on hover.
- A wrong key counts once against the letter wanted and doesn't move on. Progress per language is kept in
  `kase_settings.json` (the browser's storage on the web).

Only the keybr method itself is sourced. The numbers (6 letters, 30 wpm, 10 presses, 5%, 0.1, 2 s, 48, 12) are
choices, marked estimated in the values file.

### The game

TypingClub's stars and levels, with the keyboard's own numbers where it has them.

- Stars per line, 1 to 5: finished, 90% right, 97% right, three quarters of the target speed, the target speed.
- Experience: each character once per star, plus up to as much again for the longest run of keys without a
  mistake in the line (full bonus at 100). Levels use the keyboard's Tamagotchi rule, `level x 250 + 500`
  experience to the next one (read in the original at `0x26a99e`, as on the Typing tab).
- The combo counts keys without a mistake across lines and shows from 10, warming from cyan to orange.
- Daily goal 10 lines. A streak counts days with at least one line (UTC days) and lasts until a whole day passes
  without one. 15 badges, each earned once: first line, five stars, a spotless line, combos of 50 and 200, 40,
  60 and 80 wpm, full alphabet, every sign known, daily goal, 3 and 7 days, 100 lines, level 10.
- All of it is kept in the settings file with the rest of the Practice progress; tests never touch that file.

### Sprint and progress chart

- Sprint, as on Monkeytype's timed tests: 60 seconds of real words, the clock starting with the first key. Its
  speed counts the right keys over the minute; it earns stars and experience like a line, keeps a record and a
  Sprinter badge, and has its own record per language. Lines end when typed; the clock ends the sprint.
- Words and Sprint lines are only real words from the list once 12 fit; Letters mixes made-up words in.
- A chart shows the speed of the latest 60 lines of the mode against the target, as keybr's progress graphs do.

### Bigrams

As ngram-type drills bigrams: every pair of letters is timed on its second key, in every mode, and kept with the
letters (`pairs`). The Bigrams mode trains 5 pairs: the slowest measured ones not known yet (3 times at least),
then the language's common pairs from the word list. Pairs typed by one finger on this keymap come first and are
outlined in orange: two different base-layer keys under the same finger of the same hand, by the finger rule
above. A line is real words holding those pairs, or the pair doubled when no word has it. On the real Niphargus
in French, the five were ui, ux, vr, ix and mb, all one-finger pairs.

### Capitals, punctuation and slips

- Capitals & punctuation (Words and Sprint): sentences of 3 to 6 words, a capital to start, a comma now and then
  (1 in 6 words) and a full stop to end, each only if the keymap types it. The Shift for capitals is found on the
  base layer: a plain Shift (`0xE1`, `0xE5`) first, then a one-shot Shift (`0x3000` with `0x22`), then a mod-tap
  with Shift (a home-row mod). It lights in purple with its finger, like a layer key. On the real Niphargus it is
  LShift, left thumb.
- A capital counts for its letter in the statistics and pairs; a small letter where a capital is wanted is a miss.
- Slips: each wrong key is kept once per place as "wanted, typed" ("tr": r typed for t). The ones made twice or
  more are listed, the most frequent first.

## Settings file

The original keeps `kase_settings.json` next to the program, or in `.` (function `0x36cdc5`). An install in a
read-only folder (Nix store, `/usr`) lost every save there, the Practice progress included. It now lives in the
user's config folder: `$XDG_CONFIG_HOME` or `~/.config` on Linux, `%APPDATA%` on Windows,
`~/Library/Application Support` on macOS, in `kesp-controller/`. The old file is read when the new one doesn't
exist yet, and saving falls back to it if the folder can't be written. Tests use a temporary folder.

## Safety

- Writing a configuration file to the keyboard asks first.
- The macro editor warns when several keys have no pause between them: the firmware then holds them together
  as one shortcut (`key_processor.c`, `expand_macro`: a macro with no delay step is played as held keys).
- While disconnected, the desktop app looks at the ports every 2 s and connects to one that wasn't there at the
  last look. Ports present at start, or after a Disconnect, are not retried.
