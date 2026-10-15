use serde::Deserialize;
use serde_json::{json, Value};

use super::{board::Filter, Query, TrelloClient};
use crate::{error::Result, model::Card};

/// Card collections return at most this many per request.
pub const PAGE_SIZE: usize = 1000;
/// Hard stop for paging one board (Trello caps open cards at 5000).
pub const MAX_PAGES: usize = 10;

/// Board cards with custom field values, for a `/1/batch` url or a GET.
pub fn board_cards_url(board_id: &str, filter: Filter) -> String {
    format!(
        "/boards/{board_id}/cards/{}?customFieldItems=true&limit={PAGE_SIZE}",
        filter.as_str()
    )
}

#[derive(Deserialize)]
struct SearchCards {
    #[serde(default)]
    cards: Vec<Card>,
}

impl TrelloClient {
    /// `id` may be an id or shortLink. `full` adds checklists and attachments.
    pub async fn card(&self, id: &str, full: bool) -> Result<Card> {
        let mut q = Query::new()
            .push("fields", "all")
            .push("customFieldItems", "true");
        if full {
            q = q
                .push("checklists", "all")
                .push("attachments", "true")
                .push(
                    "attachment_fields",
                    "id,name,url,bytes,mimeType,date,isUpload",
                );
        }
        self.get(&format!("/cards/{id}"), &q).await
    }

    /// One page of a board's cards; `before` is a card id for paging back.
    pub async fn board_cards_page(
        &self,
        board_id: &str,
        filter: Filter,
        before: Option<&str>,
    ) -> Result<Vec<Card>> {
        let q = Query::new()
            .push("customFieldItems", "true")
            .push("limit", PAGE_SIZE)
            .push_opt("before", before);
        self.get(&format!("/boards/{board_id}/cards/{}", filter.as_str()), &q)
            .await
    }

    /// Every card of a board, paging back by id while pages are full.
    pub async fn board_cards(&self, board_id: &str, filter: Filter) -> Result<Vec<Card>> {
        let first = self.board_cards_page(board_id, filter, None).await?;
        self.page_rest(board_id, filter, first).await
    }

    /// Continue paging after `first` (e.g. a page that came from `/1/batch`).
    pub async fn page_rest(
        &self,
        board_id: &str,
        filter: Filter,
        first: Vec<Card>,
    ) -> Result<Vec<Card>> {
        let mut all = first;
        let mut last_len = all.len();
        for _ in 1..MAX_PAGES {
            if last_len < PAGE_SIZE {
                break;
            }
            // Ids sort by creation time, so the smallest id is the oldest.
            let Some(oldest) = all.iter().map(|c| c.id.clone()).min() else {
                break;
            };
            let page = self
                .board_cards_page(board_id, filter, Some(&oldest))
                .await?;
            last_len = page.len();
            all.extend(page);
        }
        Ok(all)
    }

    /// `(id, idShort)` of every card on a board, archived included, for
    /// resolving `board#42`.
    pub async fn short_ids(&self, board_id: &str) -> Result<Vec<(String, u32)>> {
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct Num {
            id: String,
            #[serde(default)]
            id_short: u32,
        }
        let mut all: Vec<Num> = Vec::new();
        for _ in 0..MAX_PAGES {
            let before = all.iter().map(|c| c.id.clone()).min();
            let q = Query::new()
                .push("fields", "id,idShort")
                .push("limit", PAGE_SIZE)
                .push_opt("before", before);
            let page: Vec<Num> = self
                .get(&format!("/boards/{board_id}/cards/all"), &q)
                .await?;
            let full = page.len() == PAGE_SIZE;
            all.extend(page);
            if !full {
                break;
            }
        }
        Ok(all.into_iter().map(|n| (n.id, n.id_short)).collect())
    }

    pub async fn list_cards(&self, list_id: &str) -> Result<Vec<Card>> {
        self.get(
            &format!("/lists/{list_id}/cards"),
            &Query::new().push("customFieldItems", "true"),
        )
        .await
    }

    /// Cards the token's member is on (nested route: not members-limited).
    pub async fn my_cards(&self, include_archived: bool) -> Result<Vec<Card>> {
        self.get(
            "/members/me/cards",
            &Query::new().push("filter", if include_archived { "all" } else { "open" }),
        )
        .await
    }

    /// `GET /1/search` for cards. `id_boards` narrows to boards (or `mine`).
    pub async fn search_cards(
        &self,
        query: &str,
        id_boards: &[String],
        limit: usize,
    ) -> Result<Vec<Card>> {
        let q = Query::new()
            .push("query", query)
            .push("modelTypes", "cards")
            .push("card_fields", "all")
            .push("cards_limit", limit.clamp(1, PAGE_SIZE))
            .push_opt(
                "idBoards",
                (!id_boards.is_empty()).then(|| id_boards.join(",")),
            );
        let r: SearchCards = self.get("/search", &q).await?;
        Ok(r.cards)
    }

    /// `POST /1/cards`; `body` holds `idList`, `name`, `desc`, … .
    pub async fn create_card(&self, body: Value) -> Result<Card> {
        self.post("/cards", body).await
    }

    /// `PUT /1/cards/{id}` with only the fields to change.
    pub async fn update_card(&self, id: &str, body: Value) -> Result<Card> {
        self.put(&format!("/cards/{id}"), body).await
    }

    pub async fn add_card_member(&self, id: &str, member_id: &str) -> Result<()> {
        self.post::<Value>(
            &format!("/cards/{id}/idMembers"),
            json!({ "value": member_id }),
        )
        .await
        .map(drop)
    }

    pub async fn remove_card_member(&self, id: &str, member_id: &str) -> Result<()> {
        self.delete(&format!("/cards/{id}/idMembers/{member_id}"))
            .await
            .map(drop)
    }

    pub async fn add_card_label(&self, id: &str, label_id: &str) -> Result<()> {
        self.post::<Value>(
            &format!("/cards/{id}/idLabels"),
            json!({ "value": label_id }),
        )
        .await
        .map(drop)
    }

    pub async fn remove_card_label(&self, id: &str, label_id: &str) -> Result<()> {
        self.delete(&format!("/cards/{id}/idLabels/{label_id}"))
            .await
            .map(drop)
    }

    /// Permanent. Archive (`closed: true`) is the reversible option.
    pub async fn delete_card(&self, id: &str) -> Result<()> {
        self.delete(&format!("/cards/{id}")).await.map(drop)
    }
}
