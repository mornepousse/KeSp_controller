#!/usr/bin/env python3
"""Turn the firmware's board files into app/boards/*.json.

    import_boards.py <firmware folder> <output folder>

Reads boards/*layout.inc and boards/*/board_keymap.c, read-only. Run it again
when a board changes in the firmware: the app embeds the result at build time.
"""
import json, os, re, sys

FW, OUT = sys.argv[1], sys.argv[2]

# (id, shown name, product name, layout file, keymap file, keep the right half)
BOARDS = [
    ("kase_v2", "KaSe V2", "KaSe V2", "boards/kase_layout.inc", "boards/kase_v2/board_keymap.c", True),
    ("kase_v1", "KaSe V1", "KaSe V1", "boards/kase_v1/kase_v1_layout.inc", "boards/kase_v1/board_keymap.c", True),
    ("niphargus", "Niphargus", "Niphargus Left", "boards/niphar_layout.inc", "boards/niphar_left/board_keymap.c", True),
    ("niphargus_right", "Niphargus Right (alone)", "Niphargus Right", "boards/niphar_layout.inc", "boards/niphar_right/board_keymap.c", False),
]
SIZES = {
    "kase_v2": (5, 13), "kase_v1": (5, 13), "niphargus": (4, 14), "niphargus_right": (4, 7),
}


def read(rel):
    with open(os.path.join(FW, rel), encoding="utf-8") as f:
        return f.read()


def strip_comments(s):
    s = re.sub(r"/\*.*?\*/", " ", s, flags=re.S)
    return re.sub(r"//[^\n]*", " ", s)


def key_values():
    """Name -> value, from the firmware's key definitions and TinyUSB's HID usages."""
    raw = {}
    defs = strip_comments(read("main/input/key_definitions.h"))
    hid = strip_comments(read("managed_components/espressif__tinyusb/src/class/hid/hid.h"))
    for name, val in re.findall(r"#define\s+(HID_KEY_\w+)\s+(0x[0-9A-Fa-f]+)", hid):
        raw[name] = val
    for name, val in re.findall(r"#define\s+(\w+)\s+(\w+)[ \t]*$", defs, flags=re.M):
        raw.setdefault(name, val)
    for name, val in re.findall(r"static const uint16_t\s+(\w+)\s*=\s*(0x[0-9A-Fa-f]+)", defs):
        raw[name] = val
    raw["_______"] = "K_TRNS"
    raw["XXXXXXX"] = "K_NO"

    def resolve(name, depth=0):
        if name.startswith("0x") or name.isdigit():
            return int(name, 0)
        if depth > 8 or name not in raw:
            raise KeyError(name)
        return resolve(raw[name], depth + 1)

    return resolve


def layout_json(rel, product, rows, cols, keep_right):
    src = read(rel)
    src = re.sub(r"/\*.*?\*/", " ", src, flags=re.S)
    src = src[src.index("board_layout_json"):]
    if not keep_right:
        src = re.sub(r"#if KEYMAP_COLS > MATRIX_COLS.*?#endif", "", src, flags=re.S)
    src = re.sub(r"^#.*$", "", src, flags=re.M)
    src = src.replace("PRODUCT_NAME", '"%s"' % product)
    src = src.replace("STR(MATRIX_ROWS)", '"%d"' % rows)
    src = re.sub(r"STR\((?:MATRIX|KEYMAP)_COLS\)", '"%d"' % cols, src)
    parts = re.findall(r'"((?:[^"\\]|\\.)*)"', src)
    text = "".join(parts).replace('\\"', '"')
    return json.loads(text)


def keymap_layers(rel, resolve, rows, cols):
    src = strip_comments(read(rel))
    body = src[src.index("keymaps["):]
    body = body[body.index("=") + 1:]
    body = body[: body.index(";")]
    tokens = re.findall(r"[{}]|[A-Za-z_][A-Za-z0-9_]*|0x[0-9A-Fa-f]+", body)
    layers, depth, row, layer = [], 0, [], []
    for t in tokens:
        if t == "{":
            depth += 1
        elif t == "}":
            if depth == 3:
                layer.append(row)
                row = []
            elif depth == 2:
                layers.append(layer)
                layer = []
            depth -= 1
        elif depth == 3:
            row.append(resolve(t))
    # C pads what the source leaves out: a {{XXXXXXX}} layer is all zeros.
    out = []
    for lay in layers:
        grid = [[0] * cols for _ in range(rows)]
        for r, vals in enumerate(lay[:rows]):
            for c, v in enumerate(vals[:cols]):
                grid[r][c] = v
        out.append(grid)
    return out


def layer_names(rel):
    src = strip_comments(read(rel))
    m = re.search(r"default_layout_names\[.*?=\s*\{(.*?)\};", src, flags=re.S)
    return re.findall(r'"([^"]*)"', m.group(1))


resolve = key_values()
os.makedirs(OUT, exist_ok=True)
for bid, shown, product, lay, km, right in BOARDS:
    rows, cols = SIZES[bid]
    doc = {
        "id": bid,
        "name": shown,
        "product": product,
        "layout": layout_json(lay, product, rows, cols, right),
        "layer_names": layer_names(km),
        "keymaps": keymap_layers(km, resolve, rows, cols),
    }
    with open(os.path.join(OUT, bid + ".json"), "w", encoding="utf-8") as f:
        json.dump(doc, f, ensure_ascii=False, separators=(",", ":"))
    nkeys = len(doc["layout"]["keys"]) + sum(len(g["keys"]) for g in doc["layout"].get("groups", []))
    print(f"{bid}: {nkeys} keys, {len(doc['keymaps'])} layers, first row {doc['keymaps'][0][0][:4]}")
