import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).parent.parent))

from ripblu import duration_to_seconds, sanitize_filename, parse_volume_label, filter_episodes, guess_start_episode, assign_episodes, parse_selection


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


class TestFilterEpisodes:
    def test_filters_short_playlists(self):
        playlists = [
            {"num": "00001", "duration": "0:00:30", "seconds": 30},
            {"num": "00002", "duration": "0:43:00", "seconds": 2580},
            {"num": "00003", "duration": "0:44:00", "seconds": 2640},
            {"num": "00004", "duration": "0:02:00", "seconds": 120},
        ]
        result = filter_episodes(playlists, min_duration=900)
        assert len(result) == 2
        assert result[0]["num"] == "00002"
        assert result[1]["num"] == "00003"

    def test_all_long(self):
        playlists = [
            {"num": "00001", "duration": "0:43:00", "seconds": 2580},
            {"num": "00002", "duration": "0:44:00", "seconds": 2640},
        ]
        result = filter_episodes(playlists, min_duration=900)
        assert len(result) == 2

    def test_all_short(self):
        playlists = [
            {"num": "00001", "duration": "0:00:30", "seconds": 30},
        ]
        result = filter_episodes(playlists, min_duration=900)
        assert len(result) == 0


class TestGuessStartEpisode:
    def test_disc_1(self):
        assert guess_start_episode(disc_number=1, episodes_on_disc=5) == 1

    def test_disc_2(self):
        assert guess_start_episode(disc_number=2, episodes_on_disc=5) == 6

    def test_disc_3(self):
        assert guess_start_episode(disc_number=3, episodes_on_disc=4) == 9

    def test_no_disc_number(self):
        assert guess_start_episode(disc_number=None, episodes_on_disc=5) == 1

    def test_zero_episodes(self):
        assert guess_start_episode(disc_number=2, episodes_on_disc=0) == 1


class TestAssignEpisodes:
    def test_basic_assignment(self):
        playlists = [
            {"num": "00001", "duration": "0:43:00", "seconds": 2580},
            {"num": "00002", "duration": "0:44:00", "seconds": 2640},
        ]
        episodes = [
            {"episode_number": 1, "name": "Pilot", "runtime": 44},
            {"episode_number": 2, "name": "Second", "runtime": 44},
            {"episode_number": 3, "name": "Third", "runtime": 44},
        ]
        result = assign_episodes(playlists, episodes, start_episode=1)
        assert result["00001"]["name"] == "Pilot"
        assert result["00002"]["name"] == "Second"

    def test_start_offset(self):
        playlists = [
            {"num": "00003", "duration": "0:43:00", "seconds": 2580},
        ]
        episodes = [
            {"episode_number": 1, "name": "Pilot", "runtime": 44},
            {"episode_number": 2, "name": "Second", "runtime": 44},
            {"episode_number": 3, "name": "Third", "runtime": 44},
        ]
        result = assign_episodes(playlists, episodes, start_episode=3)
        assert result["00003"]["name"] == "Third"

    def test_overflow_past_episode_list(self):
        playlists = [
            {"num": "00001", "duration": "0:43:00", "seconds": 2580},
            {"num": "00002", "duration": "0:44:00", "seconds": 2640},
        ]
        episodes = [
            {"episode_number": 1, "name": "Pilot", "runtime": 44},
        ]
        result = assign_episodes(playlists, episodes, start_episode=1)
        assert result["00001"]["name"] == "Pilot"
        assert "00002" not in result

    def test_empty_episodes(self):
        playlists = [
            {"num": "00001", "duration": "0:43:00", "seconds": 2580},
        ]
        result = assign_episodes(playlists, [], start_episode=1)
        assert result == {}


class TestParseSelection:
    def test_single_number(self):
        assert parse_selection("2", max_val=5) == [1]

    def test_comma_separated(self):
        assert parse_selection("1,3,5", max_val=5) == [0, 2, 4]

    def test_range(self):
        assert parse_selection("2-4", max_val=5) == [1, 2, 3]

    def test_mixed(self):
        assert parse_selection("1,3-5", max_val=5) == [0, 2, 3, 4]

    def test_all(self):
        assert parse_selection("all", max_val=3) == [0, 1, 2]

    def test_out_of_bounds(self):
        assert parse_selection("6", max_val=5) is None

    def test_zero(self):
        assert parse_selection("0", max_val=5) is None

    def test_invalid(self):
        assert parse_selection("abc", max_val=5) is None

    def test_empty(self):
        assert parse_selection("", max_val=5) is None

    def test_reversed_range(self):
        assert parse_selection("4-2", max_val=5) is None

    def test_open_ended_range(self):
        assert parse_selection("3-", max_val=5) == [2, 3, 4]

    def test_open_ended_range_from_1(self):
        assert parse_selection("1-", max_val=3) == [0, 1, 2]
