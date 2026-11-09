#!/usr/bin/env python3
"""Validate UI keys, catalogue parity, and reviewed untranslated names/loanwords."""
import json
from pathlib import Path
import re
import sys
import xml.etree.ElementTree as ET


ROOT = Path(__file__).resolve().parents[1]
UI_PROPERTIES = {"label", "title", "tooltip-text", "placeholder-text"}
VERSION_LABEL = re.compile(r"Open Music Player · \d+\.\d+\.\d+(?:[-+][\w.-]+)?")
# Tokenize strings and comments before punctuation so braces inside them do not
# affect the scan. This deliberately extracts literal .tr arguments/branches,
# not arbitrary runtime expressions or strings used in a branch's condition.
RUST_TOKENS = re.compile(
    r'//[^\n]*|/\*.*?\*/|r(?P<hashes>#{0,255})".*?"(?P=hashes)'
    r'|"(?:\\.|[^"\\])*"|=>|[A-Za-z_][A-Za-z_0-9]*|[^\s]',
    re.DOTALL,
)


def unique_object(pairs):
    result = {}
    for key, value in pairs:
        if key in result:
            raise ValueError(f"Duplicate JSON key: {key!r}")
        result[key] = value
    return result


def read_json(path):
    try:
        return json.loads(path.read_text(encoding="utf-8"), object_pairs_hook=unique_object)
    except ValueError as error:
        raise ValueError(f"{path.name}: {error}") from error


def string_value(token):
    if token.startswith('r'):
        return token[token.index('"') + 1:token.rindex('"')]
    # Rust's Unicode escapes and continued lines differ from JSON strings.
    token = re.sub(r"\\\n\s*", "", token)
    token = re.sub(r"\\u\{([0-9a-fA-F_]+)\}",
                   lambda m: json.dumps(chr(int(m[1].replace("_", ""), 16)))[1:-1], token)
    return json.loads(token)


def rust_translation_keys(source):
    tokens = [m[0] for m in RUST_TOKENS.finditer(source)
              if not m[0].startswith(("//", "/*"))]
    required = set()
    for start in range(len(tokens) - 3):
        is_app_call = tokens[start:start + 3] == [".", "tr", "("]
        is_i18n_call = (start > 0 and tokens[start - 1:start + 3] == ["i18n", ".", "text", "("])
        if not (is_app_call or is_i18n_call):
            continue
        depth = 1
        for i in range(start + 3, len(tokens)):
            token = tokens[i]
            if token == "(":
                depth += 1
            elif token == ")":
                depth -= 1
                if depth == 0:
                    break
            if (token.startswith('"') or re.match(r'r#*"', token)) and (
                i == start + 3 or tokens[i - 1] in {"{", "=>"}
            ) and tokens[i + 1:i + 2] not in [["=>"], ["|"]]:
                value = string_value(token)
                if value:
                    required.add(value)
    return required


def required_keys(root):
    required = set()
    for source in (root / "resources").glob("*.ui"):
        for prop in ET.parse(source).iter("property"):
            if prop.attrib.get("name") in UI_PROPERTIES and prop.text:
                required.add(prop.text)
    for source in (root / "src").rglob("*.rs"):
        required.update(rust_translation_keys(source.read_text(encoding="utf-8")))
    return required


def validate(root=ROOT):
    english = read_json(root / "locales/en.json")
    exceptions = read_json(root / "scripts/translation-exceptions.json")
    catalogues = {path.stem: read_json(path)
                  for path in sorted((root / "locales").glob("*.json"))}
    errors = []
    missing = required_keys(root) - english.keys()
    if missing:
        errors.append(f"Missing English keys: {sorted(missing)}")
    for language, allowed in exceptions.items():
        if language != "common" and language not in catalogues:
            errors.append(f"Exceptions for unknown language: {language}")
        for key, reason in allowed.items():
            if key not in english or not isinstance(reason, str) or not reason.strip():
                errors.append(f"Invalid exception in {language}: {key!r}")
            elif language != "common" and catalogues.get(language, {}).get(key) != english[key]:
                errors.append(f"Unused exception in {language}: {key!r}")
    for language, messages in catalogues.items():
        if messages.keys() != english.keys():
            errors.append(f"Key mismatch in {language}: missing {sorted(english.keys() - messages.keys())}; "
                          f"extra {sorted(messages.keys() - english.keys())}")
        allowed = exceptions.get("common", {}).keys() | exceptions.get(language, {}).keys()
        for key, value in messages.items():
            if not isinstance(value, str) or not value.strip():
                errors.append(f"Empty or non-text translation in {language}: {key!r}")
            elif (language != "en" and value == english.get(key)
                  and key not in allowed and not VERSION_LABEL.fullmatch(key)):
                errors.append(f"Untranslated English in {language}: {key!r}")
    if errors:
        raise ValueError("\n".join(errors))
    return len(catalogues), len(english)


if __name__ == "__main__":
    try:
        count, messages = validate()
    except (ValueError, OSError, ET.ParseError) as error:
        sys.exit(str(error))
    print(f"{count} catalogues validated; {messages} messages per language; no unreviewed English placeholders.")
