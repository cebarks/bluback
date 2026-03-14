import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).parent.parent))

from ripblu import duration_to_seconds, sanitize_filename, parse_volume_label


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


class TestParseVolumeLabel:
    def test_sXdY_format(self):
        result = parse_volume_label("SGU_BR_S1D2")
        assert result == {"show": "SGU BR", "season": 1, "disc": 2}

    def test_sX_dY_underscore_separated(self):
        result = parse_volume_label("SHOW_S1_D2")
        assert result == {"show": "SHOW", "season": 1, "disc": 2}

    def test_season_disc_long_form(self):
        result = parse_volume_label("SHOW_SEASON1_DISC2")
        assert result == {"show": "SHOW", "season": 1, "disc": 2}

    def test_no_match(self):
        result = parse_volume_label("RANDOM_DISC")
        assert result == {}

    def test_empty_string(self):
        result = parse_volume_label("")
        assert result == {}

    def test_show_with_underscores_before_season(self):
        result = parse_volume_label("THE_WIRE_S3D1")
        assert result == {"show": "THE WIRE", "season": 3, "disc": 1}
