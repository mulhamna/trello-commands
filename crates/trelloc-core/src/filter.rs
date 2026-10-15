//! Card filters: plan how to fetch (board scan, my cards, or `/1/search`),
//! build Trello search operators, and evaluate the exact predicate locally.
//!
//! Search is fuzzy and eventually consistent, so its results go through the
//! same predicate as a board scan.

use chrono::{DateTime, Utc};

use crate::{
    error::{Result, TrelloError},
    model::{Card, CustomField},
    time::parse_iso,
};

#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Range {
    pub after: Option<DateTime<Utc>>,
    pub before: Option<DateTime<Utc>>,
}

impl Range {
    pub fn is_empty(&self) -> bool {
        self.after.is_none() && self.before.is_none()
    }

    /// No bounds → always true; bounds but no value → false.
    fn contains(&self, t: Option<DateTime<Utc>>) -> bool {
        if self.is_empty() {
            return true;
        }
        let Some(t) = t else { return false };
        self.after.is_none_or(|a| t > a) && self.before.is_none_or(|b| t < b)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Has {
    Attachments,
    Description,
    Members,
    Cover,
}

impl std::str::FromStr for Has {
    type Err = TrelloError;
    fn from_str(s: &str) -> Result<Self> {
        Ok(match s.trim().to_ascii_lowercase().as_str() {
            "attachments" | "attachment" => Self::Attachments,
            "description" | "desc" => Self::Description,
            "members" | "member" => Self::Members,
            "cover" => Self::Cover,
            other => {
                return Err(TrelloError::Invalid(format!(
                    "unknown --has '{other}' (attachments, description, members, cover)"
                )))
            }
        })
    }
}

impl Has {
    fn operator(self) -> &'static str {
        match self {
            Self::Attachments => "has:attachments",
            Self::Description => "has:description",
            Self::Members => "has:members",
            Self::Cover => "has:cover",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FieldOp {
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
    Contains,
    Empty,
    Set,
}

#[derive(Debug, Clone, PartialEq)]
pub struct FieldFilter {
    pub name: String,
    pub op: FieldOp,
    pub value: String,
}

/// `Severity=High`, `Points>=3`, `Notes~bug`, `Reviewed!=true`,
/// `Owner empty`, `Owner set`.
pub fn parse_field_expr(expr: &str) -> Result<FieldFilter> {
    let e = expr.trim();
    let invalid = || {
        TrelloError::Invalid(format!(
            "cannot parse field filter '{expr}' (try 'Name=value', 'Points>=3', 'Notes~text', 'Name empty', 'Name set')"
        ))
    };
    for (kw, op) in [(" empty", FieldOp::Empty), (" set", FieldOp::Set)] {
        if e.len() > kw.len() && e.to_ascii_lowercase().ends_with(kw) {
            let name = e[..e.len() - kw.len()].trim();
            if name.is_empty() {
                return Err(invalid());
            }
            return Ok(FieldFilter {
                name: name.to_string(),
                op,
                value: String::new(),
            });
        }
    }
    // First operator from the left; two-char operators win at the same spot
    // so `>=` isn't read as `>`.
    const OPS: [(&str, FieldOp); 7] = [
        ("!=", FieldOp::Ne),
        ("<=", FieldOp::Le),
        (">=", FieldOp::Ge),
        ("=", FieldOp::Eq),
        ("<", FieldOp::Lt),
        (">", FieldOp::Gt),
        ("~", FieldOp::Contains),
    ];
    for (i, _) in e.char_indices() {
        if let Some((tok, op)) = OPS.iter().find(|(tok, _)| e[i..].starts_with(tok)) {
            let name = e[..i].trim();
            if name.is_empty() {
                return Err(invalid());
            }
            return Ok(FieldFilter {
                name: name.to_string(),
                op: *op,
                value: e[i + tok.len()..].trim().to_string(),
            });
        }
    }
    Err(invalid())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SortKey {
    Due,
    Created,
    Activity,
    #[default]
    Pos,
}

impl std::str::FromStr for SortKey {
    type Err = TrelloError;
    fn from_str(s: &str) -> Result<Self> {
        Ok(match s.trim().to_ascii_lowercase().as_str() {
            "due" => Self::Due,
            "created" => Self::Created,
            "activity" | "edited" | "updated" => Self::Activity,
            "pos" | "position" => Self::Pos,
            other => {
                return Err(TrelloError::Invalid(format!(
                    "unknown sort '{other}' (due, created, activity, pos)"
                )))
            }
        })
    }
}

/// How cards will be fetched.
#[derive(Debug, Clone, PartialEq)]
pub enum Plan {
    /// `GET /1/boards/{id}/cards` per board (batched), exact.
    BoardScan { boards: Vec<String> },
    /// `GET /1/members/me/cards`, exact.
    MemberCards,
    /// `GET /1/search`; may lag a few seconds behind writes.
    Search { query: String },
}

#[derive(Debug, Clone, Default)]
pub struct CardFilter {
    /// Board ids.
    pub boards: Vec<String>,
    /// List ids.
    pub lists: Vec<String>,
    /// Member ids (all must be on the card).
    pub members: Vec<String>,
    /// Usernames for search operators (parallel to `members`).
    pub member_names: Vec<String>,
    /// Label names or colors (all must be on the card).
    pub labels: Vec<String>,
    pub due: Range,
    pub created: Range,
    pub activity: Range,
    pub overdue: bool,
    pub no_due: bool,
    pub due_complete: Option<bool>,
    pub include_archived: bool,
    pub has: Vec<Has>,
    pub fields: Vec<FieldFilter>,
    pub text: Option<String>,
    pub raw_query: Option<String>,
    /// `--mine` without a board: use the member-cards endpoint.
    pub mine: bool,
    pub sort: SortKey,
    pub reverse: bool,
    pub limit: Option<usize>,
}

fn quote(s: &str) -> String {
    if s.contains(char::is_whitespace) {
        format!("\"{}\"", s.replace('"', ""))
    } else {
        s.to_string()
    }
}

impl CardFilter {
    pub fn plan(&self) -> Plan {
        if self.raw_query.is_some() {
            return Plan::Search {
                query: self.to_search_query(),
            };
        }
        if !self.boards.is_empty() {
            return Plan::BoardScan {
                boards: self.boards.clone(),
            };
        }
        if self.mine {
            return Plan::MemberCards;
        }
        Plan::Search {
            query: self.to_search_query(),
        }
    }

    /// Trello search operators for the parts search can express; the rest
    /// is enforced by [`CardFilter::matches`] on the results.
    pub fn to_search_query(&self) -> String {
        let mut parts: Vec<String> = Vec::new();
        if let Some(raw) = &self.raw_query {
            parts.push(raw.trim().to_string());
        }
        parts.extend(
            self.member_names
                .iter()
                .map(|m| format!("@{}", m.trim_start_matches('@'))),
        );
        parts.extend(self.labels.iter().map(|l| format!("label:{}", quote(l))));
        if self.overdue {
            parts.push("due:overdue".into());
        }
        match self.due_complete {
            Some(true) => parts.push("due:complete".into()),
            Some(false) => parts.push("due:incomplete".into()),
            None => {}
        }
        parts.extend(self.has.iter().map(|h| h.operator().to_string()));
        if let Some(t) = &self.text {
            parts.push(t.trim().to_string());
        }
        let explicit_open = self
            .raw_query
            .as_deref()
            .is_some_and(|q| q.contains("is:open") || q.contains("is:archived"));
        if !self.include_archived && !explicit_open {
            parts.push("is:open".into());
        }
        parts.retain(|p| !p.is_empty());
        parts.join(" ")
    }

    /// Exact predicate. `field` looks up a custom field definition by id.
    pub fn matches<'a>(
        &self,
        card: &Card,
        now: DateTime<Utc>,
        field: impl Fn(&str) -> Option<&'a CustomField>,
    ) -> bool {
        if card.closed && !self.include_archived {
            return false;
        }
        if !self.boards.is_empty() && !self.boards.contains(&card.id_board) {
            return false;
        }
        if !self.lists.is_empty() && !self.lists.contains(&card.id_list) {
            return false;
        }
        if !self.members.iter().all(|m| card.id_members.contains(m)) {
            return false;
        }
        let has_label = |want: &str| {
            card.labels.iter().any(|l| {
                l.id == want
                    || (!l.name.is_empty() && l.name.eq_ignore_ascii_case(want))
                    || l.color
                        .as_deref()
                        .is_some_and(|c| c.eq_ignore_ascii_case(want))
            })
        };
        if !self.labels.iter().all(|l| has_label(l)) {
            return false;
        }
        let due = card.due.as_deref().and_then(parse_iso);
        if !self.due.contains(due) {
            return false;
        }
        if self.overdue && !(due.is_some_and(|d| d < now) && !card.due_complete) {
            return false;
        }
        if self.no_due && due.is_some() {
            return false;
        }
        if self.due_complete.is_some_and(|d| d != card.due_complete) {
            return false;
        }
        if !self.created.contains(card.created_at()) {
            return false;
        }
        let activity = card.date_last_activity.as_deref().and_then(parse_iso);
        if !self.activity.contains(activity) {
            return false;
        }
        for h in &self.has {
            let ok = match h {
                Has::Attachments => card.badges.attachments > 0 || !card.attachments.is_empty(),
                Has::Description => !card.desc.trim().is_empty(),
                Has::Members => !card.id_members.is_empty(),
                Has::Cover => card.has_cover(),
            };
            if !ok {
                return false;
            }
        }
        if let Some(t) = &self.text {
            let t = t.to_lowercase();
            if !card.name.to_lowercase().contains(&t) && !card.desc.to_lowercase().contains(&t) {
                return false;
            }
        }
        self.fields.iter().all(|f| field_matches(f, card, &field))
    }

    /// Sort then truncate to `limit`.
    pub fn order(&self, cards: &mut Vec<Card>) {
        let key_time = |c: &Card| match self.sort {
            SortKey::Due => c.due.as_deref().and_then(parse_iso),
            SortKey::Created => c.created_at(),
            SortKey::Activity => c.date_last_activity.as_deref().and_then(parse_iso),
            SortKey::Pos => None,
        };
        match self.sort {
            SortKey::Pos => cards.sort_by(|a, b| {
                (a.id_board.as_str(), a.id_list.as_str())
                    .cmp(&(b.id_board.as_str(), b.id_list.as_str()))
                    .then(
                        a.pos
                            .partial_cmp(&b.pos)
                            .unwrap_or(std::cmp::Ordering::Equal),
                    )
            }),
            // Cards without the date go last.
            _ => cards.sort_by(|a, b| match (key_time(a), key_time(b)) {
                (Some(x), Some(y)) => x.cmp(&y),
                (Some(_), None) => std::cmp::Ordering::Less,
                (None, Some(_)) => std::cmp::Ordering::Greater,
                (None, None) => std::cmp::Ordering::Equal,
            }),
        }
        if self.reverse {
            cards.reverse();
        }
        if let Some(n) = self.limit {
            cards.truncate(n);
        }
    }
}

fn field_matches<'a>(
    f: &FieldFilter,
    card: &Card,
    field: &impl Fn(&str) -> Option<&'a CustomField>,
) -> bool {
    let item = card.custom_field_items.iter().find(|i| {
        i.id_custom_field == f.name
            || field(&i.id_custom_field).is_some_and(|d| d.name.eq_ignore_ascii_case(&f.name))
    });
    let value = item
        .map(|i| i.display_value(field(&i.id_custom_field)))
        .unwrap_or_default();
    match f.op {
        FieldOp::Empty => value.is_empty(),
        FieldOp::Set => !value.is_empty(),
        _ if item.is_none() => f.op == FieldOp::Ne,
        FieldOp::Contains => value.to_lowercase().contains(&f.value.to_lowercase()),
        op => {
            let ord = compare(&value, &f.value);
            match op {
                FieldOp::Eq => ord == Some(std::cmp::Ordering::Equal),
                FieldOp::Ne => ord != Some(std::cmp::Ordering::Equal),
                FieldOp::Lt => ord == Some(std::cmp::Ordering::Less),
                FieldOp::Le => ord.is_some_and(|o| o != std::cmp::Ordering::Greater),
                FieldOp::Gt => ord == Some(std::cmp::Ordering::Greater),
                FieldOp::Ge => ord.is_some_and(|o| o != std::cmp::Ordering::Less),
                FieldOp::Contains | FieldOp::Empty | FieldOp::Set => unreachable!(),
            }
        }
    }
}

/// Numbers numerically, dates chronologically (filter value may be human,
/// e.g. `fri`), everything else case-insensitively.
fn compare(card_value: &str, filter_value: &str) -> Option<std::cmp::Ordering> {
    if let (Ok(a), Ok(b)) = (card_value.parse::<f64>(), filter_value.parse::<f64>()) {
        return a.partial_cmp(&b);
    }
    if let Some(a) = parse_iso(card_value) {
        if let Ok(b) = crate::time::parse_when(filter_value, None) {
            return Some(a.cmp(&b));
        }
    }
    Some(card_value.to_lowercase().cmp(&filter_value.to_lowercase()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Badges, CustomFieldItem, FieldOption, Label, OptionValue};
    use chrono::TimeZone;

    fn now() -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 10, 14, 12, 0, 0).unwrap()
    }

    fn card() -> Card {
        Card {
            // id timestamp 0x6a000000 = 2026-05-10
            id: "6a0000000000000000000001".into(),
            id_board: "b1".into(),
            id_list: "l1".into(),
            name: "Fix login".into(),
            desc: "Users can't sign in".into(),
            id_members: vec!["m1".into(), "m2".into()],
            labels: vec![
                Label {
                    id: "lb1".into(),
                    name: "bug".into(),
                    color: Some("red".into()),
                    ..Default::default()
                },
                Label {
                    id: "lb2".into(),
                    name: String::new(),
                    color: Some("green".into()),
                    ..Default::default()
                },
            ],
            due: Some("2026-10-10T09:00:00.000Z".into()),
            date_last_activity: Some("2026-10-13T09:00:00.000Z".into()),
            custom_field_items: vec![
                CustomFieldItem {
                    id_custom_field: "f1".into(),
                    id_value: Some("o1".into()),
                    ..Default::default()
                },
                CustomFieldItem {
                    id_custom_field: "f2".into(),
                    value: Some(serde_json::json!({"number": "5"})),
                    ..Default::default()
                },
            ],
            badges: Badges {
                attachments: 1,
                ..Default::default()
            },
            ..Default::default()
        }
    }

    fn fields() -> Vec<CustomField> {
        vec![
            CustomField {
                id: "f1".into(),
                name: "Severity".into(),
                field_type: "list".into(),
                options: vec![FieldOption {
                    id: "o1".into(),
                    value: OptionValue {
                        text: "High".into(),
                    },
                    ..Default::default()
                }],
                ..Default::default()
            },
            CustomField {
                id: "f2".into(),
                name: "Points".into(),
                field_type: "number".into(),
                ..Default::default()
            },
        ]
    }

    fn m(f: &CardFilter, c: &Card) -> bool {
        let defs = fields();
        f.matches(c, now(), |id| defs.iter().find(|d| d.id == id))
    }

    fn day(d: u32) -> Option<DateTime<Utc>> {
        Some(Utc.with_ymd_and_hms(2026, 10, d, 0, 0, 0).unwrap())
    }

    #[test]
    fn plans() {
        let scoped = CardFilter {
            boards: vec!["b1".into(), "b2".into()],
            ..Default::default()
        };
        assert_eq!(
            scoped.plan(),
            Plan::BoardScan {
                boards: vec!["b1".into(), "b2".into()]
            }
        );
        let mine = CardFilter {
            mine: true,
            members: vec!["me".into()],
            ..Default::default()
        };
        assert_eq!(mine.plan(), Plan::MemberCards);
        let q = CardFilter {
            boards: vec!["b1".into()],
            raw_query: Some("label:urgent -has:members".into()),
            ..Default::default()
        };
        assert_eq!(
            q.plan(),
            Plan::Search {
                query: "label:urgent -has:members is:open".into()
            }
        );
    }

    #[test]
    fn search_operators() {
        let f = CardFilter {
            member_names: vec!["alice".into()],
            labels: vec!["bug".into(), "needs review".into()],
            overdue: true,
            due_complete: Some(false),
            has: vec![Has::Attachments],
            text: Some("login".into()),
            ..Default::default()
        };
        assert_eq!(
            f.to_search_query(),
            "@alice label:bug label:\"needs review\" due:overdue due:incomplete has:attachments login is:open"
        );
        let archived = CardFilter {
            include_archived: true,
            raw_query: Some("@me".into()),
            ..Default::default()
        };
        assert_eq!(archived.to_search_query(), "@me");
    }

    #[test]
    fn predicate_scope_members_labels_text() {
        let c = card();
        assert!(m(&CardFilter::default(), &c));
        let lists = |l: &str| CardFilter {
            lists: vec![l.into()],
            ..Default::default()
        };
        assert!(m(&lists("l1"), &c) && !m(&lists("l2"), &c));
        let members = |v: &[&str]| CardFilter {
            members: v.iter().map(|s| s.to_string()).collect(),
            ..Default::default()
        };
        assert!(m(&members(&["m1", "m2"]), &c) && !m(&members(&["m1", "m3"]), &c));
        let labels = |v: &[&str]| CardFilter {
            labels: v.iter().map(|s| s.to_string()).collect(),
            ..Default::default()
        };
        assert!(m(&labels(&["BUG", "green"]), &c));
        assert!(!m(&labels(&["bug", "blue"]), &c));
        let text = |t: &str| CardFilter {
            text: Some(t.into()),
            ..Default::default()
        };
        assert!(m(&text("SIGN IN"), &c) && !m(&text("logout"), &c));
        let mut closed = card();
        closed.closed = true;
        assert!(!m(&CardFilter::default(), &closed));
        assert!(m(
            &CardFilter {
                include_archived: true,
                ..Default::default()
            },
            &closed
        ));
    }

    #[test]
    fn predicate_dates() {
        let c = card();
        let overdue = CardFilter {
            overdue: true,
            ..Default::default()
        };
        assert!(m(&overdue, &c));
        let mut done = card();
        done.due_complete = true;
        assert!(!m(&overdue, &done), "overdue excludes dueComplete");
        let mut future = card();
        future.due = Some("2026-10-20T00:00:00.000Z".into());
        assert!(!m(&overdue, &future));

        let no_due = CardFilter {
            no_due: true,
            ..Default::default()
        };
        assert!(!m(&no_due, &c));
        let mut none = card();
        none.due = None;
        assert!(m(&no_due, &none) && !m(&overdue, &none));

        let due = |after, before| CardFilter {
            due: Range { after, before },
            ..Default::default()
        };
        assert!(m(&due(day(9), day(11)), &c));
        assert!(!m(&due(day(11), None), &c));
        assert!(!m(&due(None, day(9)), &none), "range with no due date");

        let created = |after, before| CardFilter {
            created: Range { after, before },
            ..Default::default()
        };
        let may = Some(Utc.with_ymd_and_hms(2026, 5, 1, 0, 0, 0).unwrap());
        let june = Some(Utc.with_ymd_and_hms(2026, 6, 1, 0, 0, 0).unwrap());
        assert!(m(&created(may, june), &c));
        assert!(!m(&created(june, None), &c));

        let active = CardFilter {
            activity: Range {
                after: day(12),
                before: None,
            },
            ..Default::default()
        };
        assert!(m(&active, &c));

        let done_f = |d| CardFilter {
            due_complete: Some(d),
            ..Default::default()
        };
        assert!(m(&done_f(false), &c) && !m(&done_f(true), &c));
    }

    #[test]
    fn predicate_has_and_fields() {
        let c = card();
        let has = |h: Has| CardFilter {
            has: vec![h],
            ..Default::default()
        };
        assert!(m(&has(Has::Attachments), &c));
        assert!(m(&has(Has::Description), &c));
        assert!(m(&has(Has::Members), &c));
        assert!(!m(&has(Has::Cover), &c));

        let f = |e: &str| CardFilter {
            fields: vec![parse_field_expr(e).unwrap()],
            ..Default::default()
        };
        assert!(m(&f("severity=high"), &c));
        assert!(!m(&f("Severity=Low"), &c));
        assert!(m(&f("Severity!=Low"), &c));
        assert!(m(&f("Points>=5"), &c) && m(&f("Points>4.5"), &c));
        assert!(!m(&f("Points<5"), &c) && m(&f("Points<=5"), &c));
        assert!(m(&f("Severity~hi"), &c));
        assert!(m(&f("Owner empty"), &c) && !m(&f("Points empty"), &c));
        assert!(m(&f("Points set"), &c) && !m(&f("Owner set"), &c));
        assert!(m(&f("Owner!=x"), &c) && !m(&f("Owner=x"), &c));
    }

    #[test]
    fn field_expr_parsing() {
        let p = |s| parse_field_expr(s).unwrap();
        assert_eq!(p("Points>=3").op, FieldOp::Ge);
        assert_eq!(p("Points >= 3").value, "3");
        assert_eq!(p("Reviewed!=true").op, FieldOp::Ne);
        assert_eq!(p("Notes~a=b").op, FieldOp::Contains);
        assert_eq!(p("Owner empty").op, FieldOp::Empty);
        assert_eq!(p("Severity=High").name, "Severity");
        for bad in ["", "=x", "Points", " set", "~a"] {
            assert!(parse_field_expr(bad).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn ordering_and_limit() {
        let mk = |id: &str, due: Option<&str>| Card {
            id: id.into(),
            due: due.map(str::to_string),
            ..Default::default()
        };
        let mut cards = vec![
            mk("a", None),
            mk("b", Some("2026-10-12T00:00:00.000Z")),
            mk("c", Some("2026-10-11T00:00:00.000Z")),
        ];
        let f = CardFilter {
            sort: SortKey::Due,
            limit: Some(2),
            ..Default::default()
        };
        f.order(&mut cards);
        assert_eq!(
            cards.iter().map(|c| c.id.as_str()).collect::<Vec<_>>(),
            ["c", "b"]
        );
    }
}
