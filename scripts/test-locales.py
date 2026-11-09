#!/usr/bin/env python3
"""Regression tests for translation coverage, using tiny isolated catalogues."""
import importlib.util
import json
from pathlib import Path
import tempfile
import unittest


spec = importlib.util.spec_from_file_location("check_locales", Path(__file__).with_name("check-locales.py"))
locales = importlib.util.module_from_spec(spec)
spec.loader.exec_module(locales)


class LocaleChecks(unittest.TestCase):
    def setUp(self):
        directory = tempfile.TemporaryDirectory()
        self.addCleanup(directory.cleanup)
        self.root = Path(directory.name)
        for name in ["locales", "resources", "scripts", "src"]:
            (self.root / name).mkdir()
        self.english = {"Playback speed": "Playback speed", "Spotify": "Spotify"}
        self.french = {"Playback speed": "Vitesse de lecture", "Spotify": "Spotify"}
        self.write("locales/en.json", self.english)
        self.write("locales/fr.json", self.french)
        self.write("scripts/translation-exceptions.json", {"common": {"Spotify": "Service name."}})
        (self.root / "src/main.rs").write_text('self.tr("Playback speed");', encoding="utf-8")

    def write(self, name, value):
        (self.root / name).write_text(json.dumps(value, ensure_ascii=False), encoding="utf-8")

    def test_translations_and_reviewed_names_pass(self):
        self.assertEqual(locales.validate(self.root), (2, 2))

    def test_copied_english_is_not_a_translation(self):
        self.write("locales/fr.json", self.english)
        with self.assertRaisesRegex(ValueError, "Untranslated English in fr: 'Playback speed'"):
            locales.validate(self.root)

    def test_missing_extra_and_empty_translations_fail(self):
        for messages, diagnostic in [
            ({"Spotify": "Spotify"}, "Key mismatch"),
            ({**self.french, "Extra": "Autre"}, "Key mismatch"),
            ({**self.french, "Playback speed": "  "}, "Empty or non-text"),
            ({**self.french, "Playback speed": None}, "Empty or non-text"),
        ]:
            with self.subTest(messages=messages):
                self.write("locales/fr.json", messages)
                with self.assertRaisesRegex(ValueError, diagnostic):
                    locales.validate(self.root)

    def test_duplicate_keys_are_not_silently_overwritten(self):
        (self.root / "locales/fr.json").write_text(
            '{"Playback speed":"Vitesse de lecture","Playback speed":"Playback speed"}',
            encoding="utf-8",
        )
        with self.assertRaisesRegex(ValueError, "fr.json: Duplicate JSON key"):
            locales.validate(self.root)

    def test_window_titles_and_conditional_tooltips_are_required(self):
        (self.root / "resources/window.ui").write_text(
            '<interface><object class="GtkWindow"><property name="title">Settings</property>'
            '</object></interface>', encoding="utf-8",
        )
        (self.root / "src/main.rs").write_text(
            'self.tr(if page == "internal-page" { "Playback speed" } else { "Unavailable" });',
            encoding="utf-8",
        )
        with self.assertRaises(ValueError) as caught:
            locales.validate(self.root)
        self.assertIn("Settings", str(caught.exception))
        self.assertIn("Unavailable", str(caught.exception))
        self.assertNotIn("internal-page", str(caught.exception))

    def test_rust_literals_comments_and_match_branches(self):
        source = r'''
            // self.tr("Comment");
            /* self.tr("Another comment"); */
            self.tr(
                "Playback speed"
            );
            self.tr(match page { "internal" => "Settings", _ => "Downloads" });
            self.tr(if page == "library" { "Play" } else if empty { "Add music" } else { "" });
            self.tr(r#"Quote: "Hello" (ready)"#);
            self.tr("Line\nTwo\u{2026}");
            i18n.text("Startup error");
        '''
        self.assertEqual(locales.rust_translation_keys(source), {
            "Playback speed", "Settings", "Downloads", "Play", "Add music",
            'Quote: "Hello" (ready)', "Line\nTwo…", "Startup error",
        })

    def test_loanword_exception_does_not_exempt_other_keys_or_languages(self):
        self.write("scripts/translation-exceptions.json", {
            "common": {"Spotify": "Service name."},
            "fr": {"Playback speed": "Fixture exception only."},
        })
        self.write("locales/fr.json", self.english)
        self.assertEqual(locales.validate(self.root), (2, 2))
        self.write("locales/de.json", self.english)
        with self.assertRaisesRegex(ValueError, "Untranslated English in de"):
            locales.validate(self.root)

    def test_exceptions_need_a_reason_and_must_still_apply(self):
        for exceptions, diagnostic in [
            ({"common": {"Spotify": ""}}, "Invalid exception"),
            ({"common": {"Unknown": "Not a catalogue key."}}, "Invalid exception"),
            ({"fr": {"Playback speed": "Already translated."}}, "Unused exception"),
        ]:
            with self.subTest(exceptions=exceptions):
                self.write("scripts/translation-exceptions.json", exceptions)
                with self.assertRaisesRegex(ValueError, diagnostic):
                    locales.validate(self.root)

    def test_version_labels_do_not_allow_other_untranslated_brand_text(self):
        for key in ["Open Music Player · 1.2.3", "Open Music Player · 1.2.3-rc.1"]:
            self.english[key] = self.french[key] = key
        self.write("locales/en.json", self.english)
        self.write("locales/fr.json", self.french)
        self.assertEqual(locales.validate(self.root), (2, 4))
        key = "Open Music Player · 1.2.3 update available"
        self.english[key] = self.french[key] = key
        self.write("locales/en.json", self.english)
        self.write("locales/fr.json", self.french)
        with self.assertRaisesRegex(ValueError, "Untranslated English"):
            locales.validate(self.root)


if __name__ == "__main__":
    unittest.main()
