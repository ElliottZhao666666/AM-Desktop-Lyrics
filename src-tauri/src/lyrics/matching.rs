use crate::domain::TrackIdentity;
use ferrous_opencc::{config::BuiltinConfig, OpenCC};
use std::collections::BTreeSet;
use std::sync::OnceLock;

pub const MATCH_VERSION: u8 = 2;
pub const MAX_DURATION_DIFFERENCE_MS: u64 = 8_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MatchScore {
    pub total: u16,
    pub acceptable: bool,
}

/// 各歌词源共用：歌名和歌手是必要条件，版本冲突和明显时长冲突直接拒绝。
/// 专辑可能因单曲、合辑而不同，因此用于排序而非盲目一票否决。
pub fn score_candidate(
    target: &TrackIdentity,
    duration_ms: u64,
    candidate: &TrackIdentity,
    candidate_duration_ms: Option<u64>,
) -> MatchScore {
    let title_matches = !normalize(&target.title).is_empty()
        && normalize(&target.title) == normalize(&candidate.title);
    let target_artists = artist_names(&target.artist);
    let candidate_artists = artist_names(&candidate.artist);
    let artist_score = if target_artists.is_empty() || candidate_artists.is_empty() {
        0
    } else if target_artists == candidate_artists {
        30
    } else if target_artists.is_subset(&candidate_artists) {
        25
    } else {
        0
    };
    let versions_match = if target.album.is_some() && candidate.album.is_some() {
        version_markers(target) == version_markers(candidate)
    } else {
        let title_only = |track: &TrackIdentity| TrackIdentity {
            album: None,
            ..track.clone()
        };
        version_markers(&title_only(target)) == version_markers(&title_only(candidate))
    };
    let album_score = match (&target.album, &candidate.album) {
        (Some(a), Some(b)) if normalize(a) == normalize(b) => 10,
        (None, _) | (_, None) => 5,
        _ => 0,
    };
    let difference = candidate_duration_ms
        .filter(|value| *value > 0)
        .filter(|_| duration_ms > 0)
        .map(|value| value.abs_diff(duration_ms));
    let duration_score = match difference {
        Some(0..=1_000) => 10,
        Some(1_001..=4_000) => 8,
        Some(4_001..=8_000) => 3,
        None => 5,
        _ => 0,
    };
    let total = if title_matches { 50 } else { 0 } + artist_score + album_score + duration_score;
    MatchScore {
        total,
        acceptable: title_matches
            && artist_score >= 25
            && versions_match
            && difference.is_none_or(|value| value <= MAX_DURATION_DIFFERENCE_MS)
            && total >= 80,
    }
}

pub fn normalize(value: &str) -> String {
    static CONVERTER: OnceLock<Option<OpenCC>> = OnceLock::new();
    let converted = CONVERTER
        .get_or_init(|| OpenCC::from_config(BuiltinConfig::T2s).ok())
        .as_ref()
        .map(|converter| converter.convert(value))
        .unwrap_or_else(|| value.to_string());
    converted
        .chars()
        .filter(|ch| ch.is_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect()
}

fn artist_names(value: &str) -> BTreeSet<String> {
    value
        .to_lowercase()
        .replace(" feat. ", "/")
        .replace(" featuring ", "/")
        .replace(" and ", "/")
        .split(|ch| matches!(ch, ',' | '，' | '/' | '、' | '&' | '＆' | ';' | '；'))
        .map(normalize)
        .filter(|part| !part.is_empty())
        .collect()
}

fn version_markers(track: &TrackIdentity) -> BTreeSet<&'static str> {
    let text = format!(
        "{} {}",
        track.title,
        track.album.as_deref().unwrap_or_default()
    )
    .to_lowercase();
    let normalized = normalize(&text);
    let mut markers = BTreeSet::new();
    for (marker, names) in [
        ("live", vec!["live", "现场", "演唱会"]),
        ("remix", vec!["remix", "混音"]),
        ("instrumental", vec!["instrumental", "伴奏", "karaoke"]),
        ("acoustic", vec!["acoustic", "不插电"]),
        ("demo", vec!["demo"]),
    ] {
        if names.iter().any(|name| {
            if name.is_ascii() {
                text.split(|ch: char| !ch.is_alphanumeric())
                    .any(|part| part == *name)
            } else {
                normalized.contains(name)
            }
        }) {
            markers.insert(marker);
        }
    }
    markers
}

#[cfg(test)]
mod tests {
    use super::*;
    fn track(title: &str, artist: &str, album: Option<&str>) -> TrackIdentity {
        TrackIdentity {
            title: title.into(),
            artist: artist.into(),
            album: album.map(str::to_owned),
        }
    }
    #[test]
    fn rejects_wrong_artist_wrong_title_duration_and_versions() {
        let target = track("Song", "Artist", Some("Album"));
        for (candidate, duration) in [
            (track("Other", "Artist", Some("Album")), 180000),
            (track("Song", "Other", Some("Album")), 180000),
            (track("Song", "Artist", Some("Album")), 220000),
            (track("Song", "Artist", Some("Live")), 180000),
        ] {
            assert!(!score_candidate(&target, 180000, &candidate, Some(duration)).acceptable);
        }
    }
    #[test]
    fn handles_multi_artists_punctuation_and_traditional_chinese() {
        assert!(
            score_candidate(
                &track("愛你", "甲，乙", None),
                180000,
                &track("爱你", "乙 / 甲", None),
                Some(181000)
            )
            .acceptable
        );
        assert!(
            score_candidate(
                &track("Song", "Gareth.T", Some("Album")),
                180000,
                &track("Song", "Gareth.T / Guest", Some("Album")),
                Some(180000)
            )
            .acceptable
        );
        assert!(
            !score_candidate(
                &track("Song", "Li", None),
                180000,
                &track("Song", "Lily", None),
                Some(180000)
            )
            .acceptable
        );
    }
    #[test]
    fn ranks_album_and_duration_but_allows_compilations() {
        let target = track("Song", "Artist", Some("Album"));
        let same = score_candidate(&target, 180000, &target, Some(180000));
        let compilation = score_candidate(
            &target,
            180000,
            &track("Song", "Artist", Some("Collection")),
            Some(181000),
        );
        assert!(compilation.acceptable);
        assert!(same.total > compilation.total);
    }
}
