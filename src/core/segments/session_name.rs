use super::{Segment, SegmentData};
use crate::config::{InputData, SegmentId, TranscriptEntry};
use std::collections::HashMap;
use std::fs;
use std::io::{BufRead, BufReader};
use std::path::Path;

#[derive(Default)]
pub struct SessionNameSegment;

impl SessionNameSegment {
    pub fn new() -> Self {
        Self
    }

    fn parse_session_name(transcript_path: &str) -> Option<String> {
        let path = Path::new(transcript_path);
        let file = fs::File::open(path).ok()?;
        let reader = BufReader::new(file);

        let mut last_title: Option<String> = None;

        for line in reader.lines() {
            let line = match line {
                Ok(l) => l,
                Err(_) => continue,
            };
            let line = line.trim();
            if line.is_empty() {
                continue;
            }

            let entry: TranscriptEntry = match serde_json::from_str(line) {
                Ok(e) => e,
                Err(_) => continue,
            };

            if entry.r#type.as_deref() == Some("custom-title") {
                if let Some(title) = &entry.title {
                    if !title.is_empty() {
                        last_title = Some(title.clone());
                    }
                }
            }
        }

        last_title
    }
}

impl Segment for SessionNameSegment {
    fn collect(&self, input: &InputData) -> Option<SegmentData> {
        let name = Self::parse_session_name(&input.transcript_path)?;

        let mut metadata = HashMap::new();
        metadata.insert("session_name".to_string(), name.clone());

        Some(SegmentData {
            primary: name,
            secondary: String::new(),
            metadata,
        })
    }

    fn id(&self) -> SegmentId {
        SegmentId::SessionName
    }
}
