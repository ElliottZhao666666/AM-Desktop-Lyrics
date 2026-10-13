use super::models::{LyricLine, LyricWord};
use std::error::Error;
/// 兼容旧调用者的模型适配层，所有时间/文本规则由唯一 TTML 解析器处理。
pub fn parse_ttml(xml: &str) -> Result<Vec<LyricLine>, Box<dyn Error + Send + Sync>> {
    parse_ttml_with_translation(xml, true)
}
pub fn parse_ttml_with_translation(
    xml: &str,
    prefer_translation: bool,
) -> Result<Vec<LyricLine>, Box<dyn Error + Send + Sync>> {
    let document = super::ttml::parse_ttml_with_translation(
        crate::domain::TrackIdentity::default(),
        crate::domain::LyricsSource::AppleMusicApi,
        xml,
        prefer_translation,
    )?;
    Ok(document
        .lines
        .into_iter()
        .map(|line| LyricLine {
            begin: line.begin_ms as f64,
            end: line.end_ms.map(|end| end as f64),
            text: line.text,
            words: (!line.words.is_empty()).then(|| {
                line.words
                    .into_iter()
                    .map(|word| LyricWord {
                        text: word.text,
                        begin: word.begin_ms as f64,
                        end: word.end_ms as f64,
                    })
                    .collect()
            }),
        })
        .collect())
}
