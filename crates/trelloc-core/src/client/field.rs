use serde_json::{json, Value};

use super::TrelloClient;
use crate::{
    error::{Result, TrelloError},
    model::CustomField,
    time::{is_none, when_iso},
};

/// Body for `PUT /1/cards/{idCard}/customField/{idField}/item`
/// (TRELLO-API §10). Numbers and checkboxes travel as strings.
pub fn field_value_body(field: &CustomField, raw: &str, tz: Option<&str>) -> Result<Value> {
    let raw = raw.trim();
    let invalid = |what: &str| {
        TrelloError::Invalid(format!(
            "'{raw}' is not a valid {what} for field '{}'",
            field.name
        ))
    };
    Ok(match field.field_type.as_str() {
        "list" => {
            let opt = field
                .options
                .iter()
                .find(|o| o.id == raw || o.value.text.eq_ignore_ascii_case(raw))
                .ok_or_else(|| {
                    let names: Vec<String> =
                        field.options.iter().map(|o| o.value.text.clone()).collect();
                    TrelloError::resolve(
                        "dropdown option",
                        raw,
                        crate::resolve::suggest(raw, &names)
                            .into_iter()
                            .chain(names.iter().take(10).cloned())
                            .take(10)
                            .collect(),
                    )
                })?;
            json!({ "idValue": opt.id })
        }
        "number" => {
            let n: f64 = raw.parse().map_err(|_| invalid("number"))?;
            if !n.is_finite() {
                return Err(invalid("number"));
            }
            json!({ "value": { "number": raw } })
        }
        "checkbox" => {
            let b = match raw.to_ascii_lowercase().as_str() {
                "true" | "yes" | "y" | "1" | "on" | "checked" | "x" => true,
                "false" | "no" | "n" | "0" | "off" | "unchecked" | "" => false,
                _ => return Err(invalid("checkbox value (true/false)")),
            };
            json!({ "value": { "checked": b.to_string() } })
        }
        "date" => json!({ "value": { "date": when_iso(raw, tz)? } }),
        _ => json!({ "value": { "text": raw } }),
    })
}

/// Body that clears a field (values can't be DELETEd).
pub fn field_clear_body(field: &CustomField) -> Value {
    if field.field_type == "list" {
        json!({ "idValue": "" })
    } else {
        json!({ "value": "" })
    }
}

/// True when the raw input means "clear this field".
pub fn is_clear(raw: &str) -> bool {
    is_none(raw)
}

fn power_up_hint(e: TrelloError) -> TrelloError {
    match e {
        TrelloError::Api {
            status: 403,
            message,
            ..
        } => TrelloError::Unavailable(format!(
            "{message} — custom fields need the Custom Fields Power-Up enabled on the board"
        )),
        other => other,
    }
}

impl TrelloClient {
    pub async fn set_card_field(&self, card_id: &str, field_id: &str, body: Value) -> Result<()> {
        self.put::<Value>(
            &format!("/cards/{card_id}/customField/{field_id}/item"),
            body,
        )
        .await
        .map(drop)
        .map_err(power_up_hint)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{FieldOption, OptionValue};

    fn field(t: &str) -> CustomField {
        CustomField {
            id: "f".into(),
            name: "F".into(),
            field_type: t.into(),
            options: vec![FieldOption {
                id: "o1".into(),
                value: OptionValue {
                    text: "High".into(),
                },
                ..Default::default()
            }],
            ..Default::default()
        }
    }

    #[test]
    fn bodies_match_trello_api_section_10() {
        let b = |t: &str, v: &str| field_value_body(&field(t), v, Some("UTC")).unwrap();
        assert_eq!(b("text", "hello"), json!({ "value": { "text": "hello" } }));
        assert_eq!(b("number", "42"), json!({ "value": { "number": "42" } }));
        assert_eq!(
            b("checkbox", "yes"),
            json!({ "value": { "checked": "true" } })
        );
        assert_eq!(
            b("checkbox", "false"),
            json!({ "value": { "checked": "false" } })
        );
        assert_eq!(
            b("date", "2026-10-20 09:00"),
            json!({ "value": { "date": "2026-10-20T09:00:00.000Z" } })
        );
        assert_eq!(b("list", "high"), json!({ "idValue": "o1" }));
        assert_eq!(field_clear_body(&field("list")), json!({ "idValue": "" }));
        assert_eq!(field_clear_body(&field("text")), json!({ "value": "" }));
        assert!(field_value_body(&field("number"), "many", None).is_err());
        assert!(field_value_body(&field("number"), "NaN", None).is_err());
        assert!(field_value_body(&field("checkbox"), "maybe", None).is_err());
        assert!(matches!(
            field_value_body(&field("list"), "Hgh", None),
            Err(TrelloError::Resolve { suggestions, .. }) if suggestions.contains(&"High".to_string())
        ));
    }
}
