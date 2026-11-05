#!/usr/bin/env python3
"""Check complete catalogue parity and all literal GTK/Rust translation keys."""
import json
from pathlib import Path
import re
import xml.etree.ElementTree as ET

root = Path(__file__).resolve().parents[1]
english = json.loads((root / "locales/en.json").read_text())
required = set()
for source in (root / "resources").glob("*.ui"):
    for prop in ET.parse(source).iter("property"):
        if prop.attrib.get("name") in {"label", "tooltip-text", "placeholder-text"} and prop.text:
            required.add(prop.text)
for source in (root / "src").glob("*.rs"):
    required.update(re.findall(r'\.tr\("([^"]+)"\)', source.read_text()))
assert required <= english.keys(), f"Missing English keys: {required - english.keys()}"
for locale in (root / "locales").glob("*.json"):
    messages = json.loads(locale.read_text())
    assert messages.keys() == english.keys(), f"Key mismatch in {locale.name}"
    assert all(isinstance(v, str) and v.strip() for v in messages.values()), locale.name
print(f"10 catalogues validated; {len(english)} messages per language.")
