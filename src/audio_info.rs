use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Details {
    pub codec: Option<String>,
    pub bitrate: Option<u64>,
    pub bitrate_estimated: bool,
    pub sample_rate: Option<u32>,
    pub bit_depth: Option<u32>,
}

impl Details {
    pub fn from_probe(data: &serde_json::Value) -> Self {
        let Some(stream) = data["streams"]
            .as_array()
            .and_then(|streams| streams.iter().find(|s| s["codec_type"] == "audio"))
        else {
            return Self::default();
        };
        let bitrate = number(&stream["bit_rate"]);
        // Container bitrate is only an estimate; never use it for video files.
        let estimate = data["streams"]
            .as_array()
            .filter(|streams| {
                streams
                    .iter()
                    .filter(|s| s["codec_type"] == "audio")
                    .count()
                    == 1
                    && !streams.iter().any(|s| {
                        s["codec_type"] == "video" && s["disposition"]["attached_pic"] != 1
                    })
            })
            .and_then(|_| number(&data["format"]["bit_rate"]));
        Self {
            codec: stream["codec_name"]
                .as_str()
                .filter(|s| !s.is_empty())
                .map(str::to_string),
            bitrate: bitrate.or(estimate),
            bitrate_estimated: bitrate.is_none() && estimate.is_some(),
            sample_rate: number(&stream["sample_rate"]).and_then(|n| n.try_into().ok()),
            // Decoder sample_fmt is not source bit depth (MP3 commonly decodes to float).
            bit_depth: number(&stream["bits_per_raw_sample"])
                .or_else(|| number(&stream["bits_per_sample"]))
                .and_then(|n| n.try_into().ok()),
        }
    }

    pub fn fill_missing(&mut self, other: &Self) {
        if self.codec.is_none() {
            self.codec.clone_from(&other.codec);
        }
        if self.bitrate.is_none() {
            self.bitrate = other.bitrate;
            self.bitrate_estimated = other.bitrate_estimated;
        }
        self.sample_rate = self.sample_rate.or(other.sample_rate);
        self.bit_depth = self.bit_depth.or(other.bit_depth);
    }
}

fn number(value: &serde_json::Value) -> Option<u64> {
    value
        .as_u64()
        .or_else(|| value.as_str()?.parse().ok())
        .filter(|n| *n > 0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn source_precision_is_distinct_from_decoder_format() {
        let mp3 = Details::from_probe(
            &json!({"streams":[{"codec_type":"audio","codec_name":"mp3","bit_rate":"192000","sample_rate":"44100","sample_fmt":"fltp","bits_per_sample":0}]}),
        );
        assert_eq!(mp3.bitrate, Some(192_000));
        assert_eq!(mp3.sample_rate, Some(44_100));
        assert_eq!(mp3.bit_depth, None);
        let flac = Details::from_probe(
            &json!({"streams":[{"codec_type":"audio","codec_name":"flac","sample_rate":"96000","bits_per_raw_sample":"24","sample_fmt":"s32"}],"format":{"bit_rate":"1520000"}}),
        );
        assert_eq!(flac.bit_depth, Some(24));
        assert!(flac.bitrate_estimated);
        let video = Details::from_probe(
            &json!({"streams":[{"codec_type":"audio"},{"codec_type":"video"}],"format":{"bit_rate":"5000000"}}),
        );
        assert_eq!(video.bitrate, None);
    }
}
