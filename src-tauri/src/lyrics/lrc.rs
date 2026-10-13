use crate::domain::LyricLine;

pub fn parse_lrc(value: &str, duration_ms: Option<u64>) -> Vec<LyricLine> {
    let offset = value
        .lines()
        .filter_map(|line| {
            line.trim()
                .trim_start_matches('\u{feff}')
                .strip_prefix("[offset:")
                .and_then(|value| value.strip_suffix(']'))
                .and_then(|value| value.trim().parse::<i64>().ok())
        })
        .last()
        .unwrap_or(0);
    let mut lines = Vec::new();
    for raw in value.lines() {
        let mut rest = raw.trim().trim_start_matches('\u{feff}');
        let mut timestamps = Vec::new();
        while let Some(tag) = rest.strip_prefix('[').and_then(|rest| rest.split_once(']')) {
            let Some(time) = timestamp(tag.0) else { break };
            timestamps.push(time);
            rest = tag.1;
        }
        if timestamps.is_empty() {
            continue;
        }
        // 空时间标签也作为清屏边界保留，避免长间奏前一行一直占用。
        for time in timestamps {
            // LRC offset 的正值提前、负值延后，与 AMDL 自身的运行时偏移分开处理。
            let begin = (time as i128 - offset as i128).clamp(0, u64::MAX as i128) as u64;
            lines.push(LyricLine {
                begin_ms: begin,
                end_ms: None,
                words: Vec::new(),
                text: rest.trim().to_string(),
            });
        }
    }
    lines.sort_by_key(|line| line.begin_ms);
    for index in 0..lines.len() {
        lines[index].end_ms = lines[index + 1..]
            .iter()
            .find(|line| line.begin_ms > lines[index].begin_ms)
            .map(|line| line.begin_ms)
            .or(duration_ms.filter(|duration| *duration > lines[index].begin_ms));
    }
    lines.retain(|line| !line.text.is_empty());
    lines
}
fn timestamp(value: &str) -> Option<u64> {
    let (minutes, seconds) = value.split_once(':')?;
    if minutes.is_empty() || minutes.len() > 3 || !minutes.bytes().all(|byte| byte.is_ascii_digit())
    {
        return None;
    }
    let seconds = seconds.replacen(':', ".", 1);
    let seconds = seconds.parse::<f64>().ok()?;
    if !seconds.is_finite() || !(0.0..60.0).contains(&seconds) {
        return None;
    }
    Some(minutes.parse::<u64>().ok()? * 60000 + (seconds * 1000.0).round() as u64)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn repeats_lines_applies_signed_offset_and_keeps_shared_timestamps() {
        let lines = parse_lrc(
            "[offset:500]\n[00:01.00][00:03.00]Hello\n[00:01.00]翻译\n[00:05.00]Last",
            Some(8000),
        );
        assert_eq!(
            lines.iter().map(|line| line.begin_ms).collect::<Vec<_>>(),
            vec![500, 500, 2500, 4500]
        );
        assert_eq!(lines[0].text, "Hello");
        assert_eq!(lines[0].end_ms, Some(2500));
        assert_eq!(lines[1].end_ms, Some(2500));
        assert_eq!(lines[3].end_ms, Some(8000));
        assert_eq!(
            parse_lrc("[offset:-500]\n[00:01.00]A", None)[0].begin_ms,
            1500
        );
    }
    #[test]
    fn blank_tags_end_previous_line_and_invalid_numbers_are_ignored() {
        let lines = parse_lrc(
            "[00:01.00]A\n[00:03.00]\n[00:NaN]bad\n[00:-1]bad\n[00:60]bad\n[01:02:50]B",
            None,
        );
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0].end_ms, Some(3000));
        assert_eq!(lines[1].begin_ms, 62500);
    }
}
