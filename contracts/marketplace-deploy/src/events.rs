use super::*;
use serde::Serialize;

#[derive(Serialize)]
struct Event<'a, T: Serialize> {
    standard: &'static str,
    version: &'static str,
    event: &'a str,
    data: &'a T,
}

pub(crate) fn emit<T: Serialize>(event: &str, data: &T) {
    let envelope = Event {
        standard: EVENT_STANDARD,
        version: EVENT_VERSION,
        event,
        data,
    };
    env::log_str(&format!(
        "EVENT_JSON:{}",
        serde_json::to_string(&envelope).expect("Event serialization failed")
    ));
}
