use super::{Segment, SegmentData};
use crate::config::{InputData, SegmentId};
use crate::updater::UpdateState;

#[derive(Default)]
pub struct UpdateSegment;

impl UpdateSegment {
    pub fn new() -> Self {
        Self
    }
}

impl Segment for UpdateSegment {
    fn collect(&self, _input: &InputData) -> Option<SegmentData> {
        crate::log_debug!("update: loading update state");
        let update_state = UpdateState::load();

        match update_state.status_text() {
            Some(status_text) => {
                crate::log_debug!("update: status_text={:?}", status_text);
                Some(SegmentData {
                    primary: status_text,
                    secondary: String::new(),
                    metadata: std::collections::HashMap::new(),
                })
            }
            None => {
                crate::log_debug!("update: no update status available, returning None");
                None
            }
        }
    }

    fn id(&self) -> SegmentId {
        SegmentId::Update
    }
}
