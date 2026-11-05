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

## Tried on real hardware

On the real dongle (firmware v4.2.0-beta.3): connecting, and every read of the Keymap, Behaviors, Typing and
Device tabs. Changing a key on the real keyboard works, as tried by its owner.

## Not tested on real hardware

The serial code also runs against a fake firmware on a pseudo-terminal in the tests, and the window logic
against simulated mouse and keyboard. These haven't run for real in this version: saving a macro, combo, tap
dance, override or leader sequence, restoring a config (including macros with `MACRO_ADD`), OTA, flashing,
pairing, the matrix test and the resets.
