use std::fs;

#[test]
fn test_parse_tv_search_results() {
    let data: serde_json::Value =
        serde_json::from_str(&fs::read_to_string("tests/fixtures/tmdb/search_tv.json").unwrap())
            .unwrap();
    let results = data["results"].as_array().unwrap();
    assert_eq!(results.len(), 2);
    assert_eq!(results[0]["name"], "Breaking Bad");
    assert_eq!(results[0]["id"], 1399);
    assert!(results[0]["first_air_date"].is_string());
}

#[test]
fn test_parse_movie_search_results() {
    let data: serde_json::Value =
        serde_json::from_str(&fs::read_to_string("tests/fixtures/tmdb/search_movie.json").unwrap())
            .unwrap();
    let results = data["results"].as_array().unwrap();
    assert_eq!(results.len(), 2);
    assert_eq!(results[0]["title"], "The Matrix");
}

#[test]
fn test_parse_season_detail() {
    let data: serde_json::Value = serde_json::from_str(
        &fs::read_to_string("tests/fixtures/tmdb/season_detail.json").unwrap(),
    )
    .unwrap();
    let episodes = data["episodes"].as_array().unwrap();
    assert_eq!(episodes.len(), 3);
    assert_eq!(episodes[0]["episode_number"], 1);
    assert_eq!(episodes[0]["name"], "Pilot");
    assert_eq!(episodes[0]["runtime"], 58);
}
