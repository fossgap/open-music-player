//! Line-timed LRC lyrics. Positive LRC offsets advance lyrics relative to audio.

#[derive(Clone, Debug, PartialEq)]
pub struct Line {
    pub at: f64,
    pub text: String,
}

#[derive(Default, Debug)]
pub struct Lyrics {
    pub lines: Vec<Line>,
    pub plain: String,
}

impl Lyrics {
    pub fn parse(input: &str) -> Self {
        let mut result = Self::default();
        let mut plain = Vec::new();
        let mut offset = 0.0;
        for line in input.trim_start_matches('\u{feff}').lines().take(10_000) {
            let mut text = line.trim();
            let mut times = Vec::new();
            let mut metadata = false;
            while let Some(tag) = text.strip_prefix('[').and_then(|t| t.split_once(']')) {
                if let Some(time) = timestamp(tag.0) {
                    times.push(time);
                } else if let Some(value) = tag.0.strip_prefix("offset:") {
                    if let Ok(ms) = value.trim().parse::<i64>() {
                        offset = ms.clamp(-86_400_000, 86_400_000) as f64 / 1000.0;
                    }
                    metadata = true;
                } else if tag.0.split_once(':').is_some_and(|(name, _)| {
                    ["ar", "al", "ti", "au", "by", "re", "ve", "length"].contains(&name)
                }) {
                    metadata = true;
                } else {
                    break;
                }
                text = tag.1.trim_start();
            }
            if metadata && times.is_empty() && text.is_empty() {
                continue;
            }
            plain.push(text.to_string());
            for at in times {
                result.lines.push(Line {
                    at,
                    text: text.to_string(),
                });
            }
        }
        for line in &mut result.lines {
            line.at = (line.at - offset).max(0.0);
        }
        result.lines.sort_by(|a, b| a.at.total_cmp(&b.at));
        // A shared timestamp can contain translations or simultaneous vocal lines.
        let mut grouped: Vec<Line> = Vec::new();
        for line in result.lines {
            if let Some(previous) = grouped.last_mut().filter(|p| p.at == line.at) {
                if !previous.text.is_empty() && !line.text.is_empty() {
                    previous.text.push('\n');
                }
                previous.text.push_str(&line.text);
            } else {
                grouped.push(line);
            }
        }
        result.lines = grouped;
        result.plain = plain.join("\n");
        result
    }

    pub fn active(&self, position: f64) -> Option<usize> {
        if !position.is_finite() || position < 0.0 {
            return None;
        }
        self.lines
            .partition_point(|line| line.at <= position)
            .checked_sub(1)
    }
}

fn timestamp(value: &str) -> Option<f64> {
    let (minutes, seconds) = value.split_once(':')?;
    if minutes.is_empty() || !minutes.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let minutes: u32 = minutes.parse().ok()?;
    if seconds.is_empty() || !seconds.bytes().all(|b| b.is_ascii_digit() || b == b'.') {
        return None;
    }
    let seconds: f64 = seconds.parse().ok()?;
    (minutes <= 1440 && seconds.is_finite() && (0.0..60.0).contains(&seconds))
        .then_some(f64::from(minutes) * 60.0 + seconds)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lrc_offsets_repeated_timestamps_and_seeking() {
        let lyrics = Lyrics::parse(
            "\u{feff}[ar:Artist]\n[offset:+500]\n[00:01.50][00:04.5]Hello\n[00:01.500]Translation\n[00:03]\n[00:05.250]Goodbye",
        );
        assert_eq!(lyrics.lines.len(), 4);
        assert_eq!(
            lyrics.lines[0],
            Line {
                at: 1.0,
                text: "Hello\nTranslation".into()
            }
        );
        assert_eq!(lyrics.active(0.9), None);
        assert_eq!(lyrics.active(1.0), Some(0));
        assert_eq!(lyrics.active(4.8), Some(3));
        assert_eq!(lyrics.active(2.6), Some(1));
        assert_eq!(lyrics.active(f64::NAN), None);
        assert_eq!(
            Lyrics::parse("[offset:-1000]\n[00:01]Later").lines[0].at,
            2.0
        );
    }

    #[test]
    fn plain_and_malformed_lyrics_stay_readable() {
        let lyrics = Lyrics::parse("[Chorus]\nWords < & >\n[00:NaN]invalid\n[00:60]invalid");
        assert!(lyrics.lines.is_empty());
        assert!(lyrics.plain.contains("[Chorus]\nWords < & >"));
    }
}
