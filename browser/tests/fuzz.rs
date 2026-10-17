//! The extension's decisions, fed whatever a browser or a host might send:
//! a download item, a menu click, the host's frames. Run on the host, where
//! the decisions are plain Rust. `PROPTEST_CASES=200000` for a deep hunt.

use dm_catcher::decide::{
    absent_text, first_look, hand_id, menu_target, DownloadItem, MenuClick, Probe,
};
use proptest::prelude::*;
use serde_json::{json, Value};

fn any_json() -> impl Strategy<Value = Value> {
    let leaf = prop_oneof![
        Just(Value::Null),
        any::<bool>().prop_map(Value::Bool),
        any::<f64>().prop_map(|n| json!(n)),
        any::<String>().prop_map(Value::String),
        "https?://[a-z.]{0,12}/[a-z./?=]{0,12}".prop_map(Value::String),
    ];
    leaf.prop_recursive(3, 24, 6, |inner| {
        prop_oneof![
            prop::collection::vec(inner.clone(), 0..5).prop_map(Value::Array),
            prop::collection::hash_map(
                prop_oneof![
                    Just("url".to_string()),
                    Just("finalUrl".to_string()),
                    Just("filename".to_string()),
                    Just("mime".to_string()),
                    Just("fileSize".to_string()),
                    Just("totalBytes".to_string()),
                    Just("byExtensionId".to_string()),
                    Just("menuItemId".to_string()),
                    Just("linkUrl".to_string()),
                    Just("srcUrl".to_string()),
                    Just("pageUrl".to_string()),
                    Just("type".to_string()),
                    Just("protocolVersion".to_string()),
                    "[a-zA-Z]{0,10}",
                ],
                inner,
                0..8,
            )
            .prop_map(|m| Value::Object(m.into_iter().collect())),
        ]
    })
}

proptest! {
    #[test]
    fn any_download_item_is_looked_at_without_falling_over(value in any_json(), own in any::<String>()) {
        if let Ok(item) = serde_json::from_value::<DownloadItem>(value) {
            let _ = first_look(&item, &own);
        }
    }

    #[test]
    fn any_menu_click_is_answered(value in any_json()) {
        if let Ok(click) = serde_json::from_value::<MenuClick>(value) {
            let _ = menu_target(&click);
        }
    }

    #[test]
    fn any_frame_from_the_host_is_taken_in(frames in prop::collection::vec(any_json(), 0..6), now in any::<f64>()) {
        let mut probe = Probe::default();
        for frame in &frames {
            probe.absorb(frame);
        }
        let _ = probe.has_hello();
        let _ = probe.finished();
        let _ = hand_id("dm", 3, now);
        let _ = absent_text("anything", "id", true);
    }
}
