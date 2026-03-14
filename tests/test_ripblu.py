import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).parent.parent))

from ripblu import duration_to_seconds, sanitize_filename


class TestDurationToSeconds:
    def test_hms(self):
        assert duration_to_seconds("1:23:45") == 5025

    def test_ms(self):
        assert duration_to_seconds("23:45") == 1425

    def test_zeros(self):
        assert duration_to_seconds("0:00:00") == 0

    def test_invalid(self):
        assert duration_to_seconds("") == 0


class TestSanitizeFilename:
    def test_spaces_to_underscores(self):
        assert sanitize_filename("Hello World") == "Hello_World"

    def test_removes_special_chars(self):
        assert sanitize_filename('foo/bar:baz"qux') == "foobarbazqux"

    def test_preserves_parens(self):
        assert sanitize_filename("Earth (Part 1)") == "Earth_(Part_1)"
