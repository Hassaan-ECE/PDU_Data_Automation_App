use serde_json::{json, Value};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EventKind {
    TestPing,
    Problem,
    Complete,
    Changeover,
    Stuck,
    Summary,
}

impl EventKind {
    fn headline(self, station_name: &str) -> String {
        match self {
            Self::TestPing => format!("🔵 Connection confirmed · {station_name}"),
            Self::Problem => format!("🔴 Problem · {station_name}"),
            Self::Complete => format!("🟢 Complete · {station_name}"),
            Self::Changeover => format!("🟡 Changeover · {station_name}"),
            Self::Stuck => format!("🟠 Stuck · {station_name}"),
            Self::Summary => format!("📊 Shift summary · {station_name}"),
        }
    }

    fn subject_label(self) -> Option<&'static str> {
        match self {
            Self::TestPing | Self::Summary => None,
            Self::Problem => Some("ISSUE"),
            Self::Complete => Some("STATUS"),
            Self::Changeover => Some("ACTION"),
            Self::Stuck => Some("CONDITION"),
        }
    }

    fn default_subject(self) -> &'static str {
        match self {
            Self::TestPing => "Notification delivery is working.",
            Self::Problem => "Automation requires attention",
            Self::Complete => "Ready for print and operator sign-off",
            Self::Changeover => "Shut down the PDU and change transformer taps for 415V",
            Self::Stuck => "No meaningful progress was detected",
            Self::Summary => "No events were recorded for this period.",
        }
    }

    fn adaptive_color(self) -> &'static str {
        match self {
            Self::Problem => "Attention",
            Self::Changeover | Self::Stuck => "Warning",
            Self::Complete => "Good",
            Self::TestPing | Self::Summary => "Accent",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NotificationEvent {
    pub kind: EventKind,
    pub unit_serial_number: Option<String>,
    pub subject: String,
    pub detail: Option<String>,
    pub current_step: Option<String>,
}

impl NotificationEvent {
    pub fn new(kind: EventKind, subject: impl Into<String>) -> Self {
        Self {
            kind,
            unit_serial_number: None,
            subject: subject.into(),
            detail: None,
            current_step: None,
        }
    }

    pub fn test_ping() -> Self {
        Self::new(EventKind::TestPing, "Notification delivery is working.")
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MessageSection {
    pub label: Option<String>,
    pub value: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NotificationMessage {
    kind: EventKind,
    headline: String,
    sections: Vec<MessageSection>,
    timestamp: String,
    text: String,
}

impl NotificationMessage {
    pub fn kind(&self) -> EventKind {
        self.kind
    }

    pub fn headline(&self) -> &str {
        &self.headline
    }

    pub fn sections(&self) -> &[MessageSection] {
        &self.sections
    }

    pub fn timestamp(&self) -> &str {
        &self.timestamp
    }

    pub fn text(&self) -> &str {
        &self.text
    }
}

pub fn format_event_message(
    station_name: &str,
    event: &NotificationEvent,
    timestamp: &str,
) -> NotificationMessage {
    let station_name = nonempty(station_name).unwrap_or("Unknown station");
    let timestamp = nonempty(timestamp)
        .map(str::to_string)
        .unwrap_or_else(now_timestamp);
    let mut sections = Vec::new();
    if let Some(serial) = optional_nonempty(event.unit_serial_number.as_deref()) {
        sections.push(labeled("UNIT", serial));
    }
    let subject = nonempty(&event.subject).unwrap_or_else(|| event.kind.default_subject());
    sections.push(MessageSection {
        label: event.kind.subject_label().map(str::to_string),
        value: subject.to_string(),
    });
    if let Some(detail) = optional_nonempty(event.detail.as_deref()) {
        sections.push(labeled("DETAIL", detail));
    }
    if let Some(step) = optional_nonempty(event.current_step.as_deref()) {
        sections.push(labeled(
            if event.kind == EventKind::Changeover {
                "NEXT STEP"
            } else {
                "CURRENT STEP"
            },
            step,
        ));
    }

    let headline = event.kind.headline(station_name);
    let mut text_parts = vec![headline.clone()];
    text_parts.extend(sections.iter().map(|section| match &section.label {
        Some(label) => format!("{label}\n{}", section.value),
        None => section.value.clone(),
    }));
    text_parts.push(timestamp.clone());

    NotificationMessage {
        kind: event.kind,
        headline,
        sections,
        timestamp,
        text: text_parts.join("\n\n"),
    }
}

pub fn format_event_message_now(
    station_name: &str,
    event: &NotificationEvent,
) -> NotificationMessage {
    format_event_message(station_name, event, &now_timestamp())
}

pub fn now_timestamp() -> String {
    chrono::Local::now()
        .format("%b %-d, %Y · %-I:%M %p")
        .to_string()
}

/// Build the proven Teams Workflow envelope with both the plain-text fallback
/// and the Adaptive Card attachment.
pub fn build_teams_payload(message: &NotificationMessage) -> Value {
    let card_body = if message.kind == EventKind::Summary {
        summary_card_body(message)
    } else {
        structured_card_body(message)
    };
    envelope(message.text(), card_body)
}

/// Compatibility builder for callers that only have already-formatted text.
pub fn build_payload(body_text: &str) -> Value {
    let sections: Vec<&str> = body_text.split("\n\n").collect();
    let title = sections.first().copied().unwrap_or(body_text);
    let timestamp = sections.last().copied().unwrap_or("");
    let color = adaptive_color_from_headline(title);
    let mut body = vec![title_block(title, color)];
    if sections.len() > 2 {
        for section in &sections[1..sections.len() - 1] {
            body.push(section_value(section));
        }
    }
    if sections.len() > 1 {
        body.push(timestamp_block(timestamp));
    }
    envelope(body_text, body)
}

fn structured_card_body(message: &NotificationMessage) -> Vec<Value> {
    let mut body = vec![title_block(
        message.headline(),
        message.kind.adaptive_color(),
    )];
    body.extend(message.sections.iter().map(section_json));
    body.push(timestamp_block(message.timestamp()));
    body
}

fn summary_card_body(message: &NotificationMessage) -> Vec<Value> {
    let mut details = message
        .sections
        .iter()
        .map(|section| match &section.label {
            Some(label) => format!("{label}\n{}", section.value),
            None => section.value.clone(),
        })
        .collect::<Vec<_>>();
    details.push(message.timestamp.clone());
    vec![
        title_block(message.headline(), message.kind.adaptive_color()),
        json!({
            "type": "TextBlock",
            "text": details.join("\n\n"),
            "wrap": true,
            "spacing": "Medium"
        }),
    ]
}

fn envelope(text: &str, card_body: Vec<Value>) -> Value {
    json!({
        "text": text,
        "type": "message",
        "attachments": [{
            "contentType": "application/vnd.microsoft.card.adaptive",
            "content": {
                "$schema": "http://adaptivecards.io/schemas/adaptive-card.json",
                "type": "AdaptiveCard",
                "version": "1.2",
                "body": card_body,
                "msteams": { "width": "Full" }
            }
        }]
    })
}

fn title_block(title: &str, color: &str) -> Value {
    json!({
        "type": "TextBlock", "text": title, "wrap": true,
        "size": "Medium", "weight": "Bolder", "color": color
    })
}

fn timestamp_block(timestamp: &str) -> Value {
    json!({
        "type": "TextBlock", "text": timestamp, "wrap": true,
        "size": "Small", "isSubtle": true, "separator": true,
        "spacing": "Medium"
    })
}

fn section_json(section: &MessageSection) -> Value {
    match &section.label {
        Some(label) => json!({
            "type": "Container", "spacing": "Medium", "items": [
                {"type":"TextBlock","text":label,"wrap":true,"size":"Small","weight":"Bolder","isSubtle":true},
                {"type":"TextBlock","text":section.value,"wrap":true,"spacing":"None"}
            ]
        }),
        None => json!({
            "type":"TextBlock","text":section.value,"wrap":true,
            "spacing":"Medium","isSubtle":true
        }),
    }
}

fn section_value(section: &str) -> Value {
    match section.split_once('\n') {
        Some((label, value)) => section_json(&MessageSection {
            label: Some(label.to_string()),
            value: value.to_string(),
        }),
        None => section_json(&MessageSection {
            label: None,
            value: section.to_string(),
        }),
    }
}

fn adaptive_color_from_headline(headline: &str) -> &'static str {
    if headline.starts_with('🔴') {
        "Attention"
    } else if headline.starts_with('🟠') {
        "Warning"
    } else if headline.starts_with('🟢') {
        "Good"
    } else if headline.starts_with('🔵') || headline.starts_with('📊') {
        "Accent"
    } else {
        "Default"
    }
}

fn labeled(label: &str, value: &str) -> MessageSection {
    MessageSection {
        label: Some(label.to_string()),
        value: value.to_string(),
    }
}

fn nonempty(value: &str) -> Option<&str> {
    let value = value.trim();
    (!value.is_empty()).then_some(value)
}

fn optional_nonempty(value: Option<&str>) -> Option<&str> {
    value.and_then(nonempty)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn problem_uses_real_subject_detail_and_current_step() {
        let event = NotificationEvent {
            kind: EventKind::Problem,
            unit_serial_number: Some("262343000072".into()),
            subject: "Workbook locked".into(),
            detail: Some("Close the report in Excel and rerun.".into()),
            current_step: Some("STEP31 · 208V Breaker 3".into()),
        };
        let message = format_event_message("Test Station 2", &event, "Jul 10, 2026 · 9:00 AM");
        assert!(message.text().starts_with("🔴 Problem · Test Station 2"));
        assert!(message.text().contains("ISSUE\nWorkbook locked"));
        assert!(message.text().contains("DETAIL\nClose the report"));
        assert!(message
            .text()
            .contains("CURRENT STEP\nSTEP31 · 208V Breaker 3"));
        assert!(!message.text().contains("STEP15"));
    }

    #[test]
    fn blank_optional_values_are_omitted() {
        let mut event = NotificationEvent::new(EventKind::Complete, "Ready for print");
        event.detail = Some("   ".into());
        event.current_step = Some("".into());
        let message = format_event_message("Test Station 1", &event, "now");
        assert_eq!(message.sections().len(), 1);
        assert_eq!(message.sections()[0].label.as_deref(), Some("STATUS"));
    }

    #[test]
    fn payload_has_root_text_and_structured_adaptive_card() {
        let mut event = NotificationEvent::new(EventKind::Complete, "Ready for print");
        event.unit_serial_number = Some("262343000072".into());
        let message = format_event_message("Test Station 3", &event, "Jul 10, 2026 · 8:29 AM");
        let payload = build_teams_payload(&message);
        assert_eq!(payload["text"], message.text());
        assert_eq!(payload["type"], "message");
        assert_eq!(payload["attachments"].as_array().unwrap().len(), 1);
        assert_eq!(payload["attachments"][0]["content"]["version"], "1.2");
        assert_eq!(
            payload["attachments"][0]["content"]["msteams"]["width"],
            "Full"
        );
        assert!(payload["attachments"][0].get("contentUrl").is_none());
        let body = &payload["attachments"][0]["content"]["body"];
        assert_eq!(body[0]["color"], "Good");
        assert_eq!(body[1]["items"][0]["text"], "UNIT");
        assert_eq!(body[2]["items"][0]["text"], "STATUS");
        assert_eq!(body[3]["separator"], true);
    }

    #[test]
    fn changeover_message_names_manual_and_next_steps() {
        let event = NotificationEvent {
            kind: EventKind::Changeover,
            unit_serial_number: Some("262343000072".to_string()),
            subject:
                "208V testing complete — shut down the PDU and change transformer taps for 415V"
                    .to_string(),
            detail: None,
            current_step: Some("STEP43 · 415V Transformer Check".to_string()),
        };
        let message = format_event_message("Test Station 1", &event, "Jul 14, 2026 · 1:00 PM");

        assert_eq!(message.headline(), "🟡 Changeover · Test Station 1");
        assert_eq!(message.sections()[1].label.as_deref(), Some("ACTION"));
        assert_eq!(message.sections()[2].label.as_deref(), Some("NEXT STEP"));
        let payload = build_teams_payload(&message);
        assert_eq!(
            payload["attachments"][0]["content"]["body"][0]["color"],
            "Warning"
        );
    }
}
