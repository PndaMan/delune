//! Private messages and chat rooms on delune's Soulseek account.
//!
//! Chat speaks for the one Soulseek account delune runs as, so it belongs to the
//! people who manage delune. Private conversations are kept in `chat.json` (the last
//! 500 lines each); room history lives in memory, as it does in every Soulseek client.
//! Rooms delune is in are remembered and rejoined whenever it reconnects.
//!
//! Pages learn about new lines from `GET /api/v1/soulseek/chat/events` and fetch
//! what changed.

use std::collections::{BTreeMap, VecDeque};
use std::convert::Infallible;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{SystemTime, UNIX_EPOCH};

use axum::{
    Json,
    extract::{Path as UrlPath, State},
    http::StatusCode,
    response::{
        IntoResponse, Response,
        sse::{Event, KeepAlive, Sse},
    },
};
use delune_core::api::{
    ApiError, ChatMessage, ChatOverview, ChatUpdate, ConversationSummary, Presence, RoomPerson, RoomSummary, RoomView,
};
use delune_soulseek::{ChatEvent, RoomMember, UserStatus};
use serde::{Deserialize, Serialize};
use tokio::sync::broadcast;

use crate::AppState;
use crate::accounts::CurrentUser;
use crate::store::Database;

const KEEP_PRIVATE: usize = 500;
const KEEP_ROOM: usize = 300;
const MAX_MESSAGE_CHARS: usize = 2_000;

#[derive(Debug)]
pub struct Chat {
    store: Option<Arc<Database>>,
    state: Mutex<History>,
    ids: AtomicU64,
    updates: broadcast::Sender<ChatUpdate>,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct History {
    #[serde(default)]
    conversations: BTreeMap<String, Conversation>,
    /// Rooms to be in.
    #[serde(default)]
    joined: Vec<String>,
    #[serde(skip)]
    rooms: BTreeMap<String, Room>,
    #[serde(skip)]
    available: Vec<(String, u32)>,
}

/// Private conversations kept at once.
const MAX_CONVERSATIONS: usize = 300;

#[derive(Debug, Default, Serialize, Deserialize)]
struct Conversation {
    messages: VecDeque<ChatMessage>,
    unread: u32,
}

#[derive(Debug, Default)]
struct Room {
    members: BTreeMap<String, RoomMember>,
    messages: VecDeque<ChatMessage>,
    unread: u32,
}

fn now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs())
}

fn push<T>(queue: &mut VecDeque<T>, item: T, cap: usize) {
    if queue.len() >= cap {
        queue.pop_front();
    }
    queue.push_back(item);
}

impl Default for Chat {
    fn default() -> Self {
        Self { store: None, state: Mutex::default(), ids: AtomicU64::new(1), updates: broadcast::channel(256).0 }
    }
}

impl Chat {
    #[must_use]
    pub fn open(db: &Arc<Database>) -> Self {
        let store = db.clone();
        let state: History = store.load("chat").unwrap_or_default();
        let next_id = state.conversations.values().flat_map(|c| c.messages.iter().map(|m| m.id)).max().unwrap_or(0) + 1;
        Self { store: Some(store), state: Mutex::new(state), ids: AtomicU64::new(next_id), ..Self::default() }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, History> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn save(&self, state: &History) {
        if let Some(db) = &self.store {
            db.save("chat", state);
        }
    }

    fn message(&self, from: &str, text: &str, outgoing: bool, at: u64) -> ChatMessage {
        ChatMessage {
            id: self.ids.fetch_add(1, Ordering::Relaxed),
            at,
            from: from.to_owned(),
            text: text.to_owned(),
            outgoing,
        }
    }

    /// Rooms to rejoin when delune starts.
    #[must_use]
    pub fn joined_rooms(&self) -> Vec<String> {
        self.lock().joined.clone()
    }

    /// Fold one event from the Soulseek client into the history.
    pub fn apply(&self, event: ChatEvent) {
        let mut state = self.lock();
        let update = match event {
            ChatEvent::PrivateMessage { timestamp, username, message } => {
                let line = self.message(&username, &message, false, u64::from(timestamp));
                // Anyone can message delune; keep the history to the most recent people.
                if !state.conversations.contains_key(&username) && state.conversations.len() >= MAX_CONVERSATIONS {
                    let quietest = state
                        .conversations
                        .iter()
                        .min_by_key(|(_, c)| c.messages.back().map_or(0, |m| m.at))
                        .map(|(name, _)| name.clone());
                    if let Some(name) = quietest {
                        state.conversations.remove(&name);
                    }
                }
                let conversation = state.conversations.entry(username.clone()).or_default();
                push(&mut conversation.messages, line.clone(), KEEP_PRIVATE);
                conversation.unread += 1;
                self.save(&state);
                ChatUpdate::Conversation { username, message: line }
            }
            ChatEvent::JoinedRoom { room, members } => {
                let entry = state.rooms.entry(room.clone()).or_default();
                entry.members = members.into_iter().map(|m| (m.username.clone(), m)).collect();
                ChatUpdate::Room { room, message: None }
            }
            ChatEvent::LeftRoom { room } => {
                state.rooms.remove(&room);
                ChatUpdate::Rooms
            }
            ChatEvent::RoomMessage { room, username, message } => {
                let line = self.message(&username, &message, false, now());
                let entry = state.rooms.entry(room.clone()).or_default();
                push(&mut entry.messages, line.clone(), KEEP_ROOM);
                entry.unread += 1;
                ChatUpdate::Room { room, message: Some(line) }
            }
            ChatEvent::UserJoinedRoom { room, member } => {
                state.rooms.entry(room.clone()).or_default().members.insert(member.username.clone(), member);
                ChatUpdate::Room { room, message: None }
            }
            ChatEvent::UserLeftRoom { room, username } => {
                if let Some(entry) = state.rooms.get_mut(&room) {
                    entry.members.remove(&username);
                }
                ChatUpdate::Room { room, message: None }
            }
            ChatEvent::RoomList(rooms) => {
                state.available = rooms.into_iter().map(|r| (r.name, r.users)).collect();
                state.available.sort_by_key(|(_, users)| std::cmp::Reverse(*users));
                ChatUpdate::Rooms
            }
            ChatEvent::UserStatus { .. } => return,
        };
        drop(state);
        let _ = self.updates.send(update);
    }

    /// Record a private message we sent (the server doesn't echo those back).
    fn sent(&self, own_username: &str, to: &str, text: &str) -> ChatMessage {
        let line = self.message(own_username, text, true, now());
        let mut state = self.lock();
        let conversation = state.conversations.entry(to.to_owned()).or_default();
        push(&mut conversation.messages, line.clone(), KEEP_PRIVATE);
        self.save(&state);
        drop(state);
        let _ = self.updates.send(ChatUpdate::Conversation { username: to.to_owned(), message: line.clone() });
        line
    }

    fn overview(&self) -> ChatOverview {
        let state = self.lock();
        let mut conversations: Vec<ConversationSummary> = state
            .conversations
            .iter()
            .map(|(username, c)| ConversationSummary {
                username: username.clone(),
                last: c.messages.back().cloned(),
                unread: c.unread,
            })
            .collect();
        conversations.sort_by(|a, b| b.last.as_ref().map(|m| m.at).cmp(&a.last.as_ref().map(|m| m.at)));

        let mut rooms: Vec<RoomSummary> = state
            .joined
            .iter()
            .map(|name| {
                let room = state.rooms.get(name);
                RoomSummary {
                    name: name.clone(),
                    members: room.map_or(0, |r| u32::try_from(r.members.len()).unwrap_or(u32::MAX)),
                    joined: true,
                    unread: room.map_or(0, |r| r.unread),
                }
            })
            .collect();
        rooms.extend(
            state
                .available
                .iter()
                .filter(|(name, _)| !state.joined.contains(name))
                .take(100)
                .map(|(name, users)| RoomSummary { name: name.clone(), members: *users, joined: false, unread: 0 }),
        );
        ChatOverview { conversations, rooms }
    }
}

fn error(status: StatusCode, code: &str, message: &str) -> Response {
    (status, Json(ApiError::new(code, message))).into_response()
}

fn guard(app: &AppState, user: &CurrentUser) -> Result<delune_soulseek::Client, Box<Response>> {
    if let Some(denied) = user.refuse_unless(|p| p.manage, "use chat on delune's Soulseek account") {
        return Err(Box::new(denied));
    }
    app.soulseek.clone().ok_or_else(|| {
        Box::new(error(StatusCode::SERVICE_UNAVAILABLE, "soulseek-not-configured", "Soulseek isn't set up."))
    })
}

fn clean_text(text: &str) -> Result<String, Box<Response>> {
    let text = text.trim();
    if text.is_empty() {
        return Err(Box::new(error(StatusCode::BAD_REQUEST, "empty", "Type a message first.")));
    }
    if text.chars().count() > MAX_MESSAGE_CHARS {
        return Err(Box::new(error(StatusCode::BAD_REQUEST, "too-long", "That message is too long to send.")));
    }
    Ok(text.to_owned())
}

/// Feed client chat events into the store. Call once at startup.
pub fn start(app: &AppState) {
    let Some(client) = app.soulseek.clone() else { return };
    for room in app.chat.joined_rooms() {
        // Offline at startup is expected; the client joins once it logs in.
        let _ = client.join_room(&room);
    }
    let chat = app.chat.clone();
    let mut events = client.chat_events();
    tokio::spawn(async move {
        loop {
            match events.recv().await {
                Ok(event) => chat.apply(event),
                Err(broadcast::error::RecvError::Lagged(missed)) => tracing::warn!(missed, "chat fell behind"),
                Err(broadcast::error::RecvError::Closed) => break,
            }
        }
    });
}

/// `GET /api/v1/soulseek/chat`
#[utoipa::path(
    get,
    operation_id = "chat_overview",
    path = "/api/v1/soulseek/chat",
    tag = "chat",
    responses(
        (status = 200, description = "OK", body = delune_core::api::ChatOverview),
        (status = 403, description = "Not allowed", body = delune_core::api::ApiError),
        (status = 401, description = "Signed out", body = delune_core::api::ApiError),
    ),
)]
pub async fn overview(State(app): State<AppState>, user: CurrentUser) -> Response {
    if let Err(response) = guard(&app, &user) {
        return *response;
    }
    Json(app.chat.overview()).into_response()
}

/// `GET /api/v1/soulseek/chat/users/{username}`: the conversation, marked read.
#[utoipa::path(
    get,
    operation_id = "chat_conversation",
    path = "/api/v1/soulseek/chat/users/{username}",
    tag = "chat",
    params(
        ("username" = String, Path),
    ),
    responses(
        (status = 200, description = "OK", body = Vec<delune_core::api::ChatMessage>),
        (status = 403, description = "Not allowed", body = delune_core::api::ApiError),
        (status = 401, description = "Signed out", body = delune_core::api::ApiError),
    ),
)]
pub async fn conversation(
    State(app): State<AppState>,
    user: CurrentUser,
    UrlPath(username): UrlPath<String>,
) -> Response {
    if let Err(response) = guard(&app, &user) {
        return *response;
    }
    let mut state = app.chat.lock();
    let messages: Vec<ChatMessage> = match state.conversations.get_mut(&username) {
        Some(conversation) => {
            let had_unread = conversation.unread > 0;
            conversation.unread = 0;
            let messages = conversation.messages.iter().cloned().collect();
            if had_unread {
                app.chat.save(&state);
            }
            messages
        }
        None => Vec::new(),
    };
    Json(messages).into_response()
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct Say {
    text: String,
}

/// `POST /api/v1/soulseek/chat/users/{username}`
#[utoipa::path(
    post,
    operation_id = "chat_send",
    path = "/api/v1/soulseek/chat/users/{username}",
    tag = "chat",
    params(
        ("username" = String, Path),
    ),
    request_body = Say,
    responses(
        (status = 201, description = "Sent", body = delune_core::api::ChatMessage),
        (status = 403, description = "Not allowed", body = delune_core::api::ApiError),
        (status = 401, description = "Signed out", body = delune_core::api::ApiError),
    ),
)]
pub async fn send(
    State(app): State<AppState>,
    user: CurrentUser,
    UrlPath(username): UrlPath<String>,
    Json(say): Json<Say>,
) -> Response {
    let client = match guard(&app, &user) {
        Ok(client) => client,
        Err(response) => return *response,
    };
    let text = match clean_text(&say.text) {
        Ok(text) => text,
        Err(response) => return *response,
    };
    if let Err(e) = client.send_message(&username, &text) {
        return error(StatusCode::SERVICE_UNAVAILABLE, "soulseek-offline", &format!("Couldn't send: {e}."));
    }
    let own = app.soulseek_username.clone().unwrap_or_default();
    tracing::info!(by = %user.username, to = %username, "private message sent");
    (StatusCode::CREATED, Json(app.chat.sent(&own, &username, &text))).into_response()
}

/// `DELETE /api/v1/soulseek/chat/users/{username}`: forget a conversation.
#[utoipa::path(
    delete,
    operation_id = "chat_forget",
    path = "/api/v1/soulseek/chat/users/{username}",
    tag = "chat",
    params(
        ("username" = String, Path),
    ),
    responses(
        (status = 204, description = "Done"),
        (status = 403, description = "Not allowed", body = delune_core::api::ApiError),
        (status = 401, description = "Signed out", body = delune_core::api::ApiError),
    ),
)]
pub async fn forget(State(app): State<AppState>, user: CurrentUser, UrlPath(username): UrlPath<String>) -> Response {
    if let Err(response) = guard(&app, &user) {
        return *response;
    }
    let mut state = app.chat.lock();
    state.conversations.remove(&username);
    app.chat.save(&state);
    StatusCode::NO_CONTENT.into_response()
}

fn presence(status: UserStatus) -> Presence {
    match status {
        UserStatus::Online => Presence::Online,
        UserStatus::Away => Presence::Away,
        UserStatus::Offline => Presence::Offline,
    }
}

/// `GET /api/v1/soulseek/chat/rooms/{room}`: members and recent lines, marked read.
#[utoipa::path(
    get,
    operation_id = "chat_room",
    path = "/api/v1/soulseek/chat/rooms/{room}",
    tag = "chat",
    params(
        ("room" = String, Path),
    ),
    responses(
        (status = 200, description = "OK", body = delune_core::api::RoomView),
        (status = 403, description = "Not allowed", body = delune_core::api::ApiError),
        (status = 401, description = "Signed out", body = delune_core::api::ApiError),
    ),
)]
pub async fn room(State(app): State<AppState>, user: CurrentUser, UrlPath(name): UrlPath<String>) -> Response {
    if let Err(response) = guard(&app, &user) {
        return *response;
    }
    let mut state = app.chat.lock();
    let joined = state.joined.contains(&name);
    let view = match state.rooms.get_mut(&name) {
        Some(room) => {
            room.unread = 0;
            RoomView {
                name: name.clone(),
                joined,
                members: room
                    .members
                    .values()
                    .map(|m| RoomPerson {
                        username: m.username.clone(),
                        presence: presence(m.status),
                        files: m.files,
                        avg_speed: m.avg_speed,
                        country: m.country.clone(),
                    })
                    .collect(),
                messages: room.messages.iter().cloned().collect(),
            }
        }
        None => RoomView { name, joined, members: vec![], messages: vec![] },
    };
    Json(view).into_response()
}

/// Soulseek's own rules for room names.
fn valid_room(name: &str) -> bool {
    !name.is_empty() && name.len() <= 24 && name.is_ascii() && name.trim() == name && !name.contains("  ")
}

/// `PUT /api/v1/soulseek/chat/rooms/{room}`: join.
#[utoipa::path(
    put,
    operation_id = "chat_join",
    path = "/api/v1/soulseek/chat/rooms/{room}",
    tag = "chat",
    params(
        ("room" = String, Path),
    ),
    responses(
        (status = 204, description = "Done"),
        (status = 403, description = "Not allowed", body = delune_core::api::ApiError),
        (status = 401, description = "Signed out", body = delune_core::api::ApiError),
    ),
)]
pub async fn join(State(app): State<AppState>, user: CurrentUser, UrlPath(name): UrlPath<String>) -> Response {
    let client = match guard(&app, &user) {
        Ok(client) => client,
        Err(response) => return *response,
    };
    if !valid_room(&name) {
        return error(
            StatusCode::BAD_REQUEST,
            "bad-room-name",
            "Room names are up to 24 plain characters, without leading, trailing or double spaces.",
        );
    }
    if let Err(e) = client.join_room(&name) {
        return error(StatusCode::SERVICE_UNAVAILABLE, "soulseek-offline", &format!("Couldn't join: {e}."));
    }
    let mut state = app.chat.lock();
    if !state.joined.contains(&name) {
        state.joined.push(name);
        app.chat.save(&state);
    }
    drop(state);
    let _ = app.chat.updates.send(ChatUpdate::Rooms);
    StatusCode::NO_CONTENT.into_response()
}

/// `DELETE /api/v1/soulseek/chat/rooms/{room}`: leave.
#[utoipa::path(
    delete,
    operation_id = "chat_leave",
    path = "/api/v1/soulseek/chat/rooms/{room}",
    tag = "chat",
    params(
        ("room" = String, Path),
    ),
    responses(
        (status = 204, description = "Done"),
        (status = 403, description = "Not allowed", body = delune_core::api::ApiError),
        (status = 401, description = "Signed out", body = delune_core::api::ApiError),
    ),
)]
pub async fn leave(State(app): State<AppState>, user: CurrentUser, UrlPath(name): UrlPath<String>) -> Response {
    let client = match guard(&app, &user) {
        Ok(client) => client,
        Err(response) => return *response,
    };
    let _ = client.leave_room(&name);
    let mut state = app.chat.lock();
    state.joined.retain(|r| *r != name);
    state.rooms.remove(&name);
    app.chat.save(&state);
    drop(state);
    let _ = app.chat.updates.send(ChatUpdate::Rooms);
    StatusCode::NO_CONTENT.into_response()
}

/// `POST /api/v1/soulseek/chat/rooms/{room}/messages`
#[utoipa::path(
    post,
    operation_id = "chat_say",
    path = "/api/v1/soulseek/chat/rooms/{room}/messages",
    tag = "chat",
    params(
        ("room" = String, Path),
    ),
    request_body = Say,
    responses(
        (status = 204, description = "Done"),
        (status = 403, description = "Not allowed", body = delune_core::api::ApiError),
        (status = 401, description = "Signed out", body = delune_core::api::ApiError),
    ),
)]
pub async fn say(
    State(app): State<AppState>,
    user: CurrentUser,
    UrlPath(name): UrlPath<String>,
    Json(say): Json<Say>,
) -> Response {
    let client = match guard(&app, &user) {
        Ok(client) => client,
        Err(response) => return *response,
    };
    let text = match clean_text(&say.text) {
        Ok(text) => text,
        Err(response) => return *response,
    };
    // The server echoes room lines back to everyone in the room, us included.
    match client.say(&name, &text) {
        Ok(()) => StatusCode::ACCEPTED.into_response(),
        Err(e) => error(StatusCode::SERVICE_UNAVAILABLE, "soulseek-offline", &format!("Couldn't send: {e}.")),
    }
}

/// `POST /api/v1/soulseek/chat/rooms`: refresh the public room list.
#[utoipa::path(
    post,
    operation_id = "chat_refresh_rooms",
    path = "/api/v1/soulseek/chat/rooms",
    tag = "chat",
    responses(
        (status = 204, description = "Done"),
        (status = 403, description = "Not allowed", body = delune_core::api::ApiError),
        (status = 401, description = "Signed out", body = delune_core::api::ApiError),
    ),
)]
pub async fn refresh_rooms(State(app): State<AppState>, user: CurrentUser) -> Response {
    let client = match guard(&app, &user) {
        Ok(client) => client,
        Err(response) => return *response,
    };
    let _ = client.request_room_list();
    StatusCode::ACCEPTED.into_response()
}

/// `GET /api/v1/soulseek/chat/events`: what changed, as it happens.
#[utoipa::path(
    get,
    operation_id = "chat_events",
    path = "/api/v1/soulseek/chat/events",
    tag = "chat",
    responses(
        (status = 200, description = "Server-sent events", body = delune_core::api::ChatUpdate, content_type = "text/event-stream"),
        (status = 403, description = "Not allowed", body = delune_core::api::ApiError),
        (status = 401, description = "Signed out", body = delune_core::api::ApiError),
    ),
)]
pub async fn events(State(app): State<AppState>, user: CurrentUser) -> Response {
    if let Err(response) = guard(&app, &user) {
        return *response;
    }
    let rx = app.chat.updates.subscribe();
    let stream = futures_util::stream::unfold(rx, |mut rx| async move {
        loop {
            match rx.recv().await {
                Ok(update) => {
                    let event = Event::default().json_data(&update).unwrap_or_else(|_| Event::default().data("{}"));
                    return Some((Ok::<_, Infallible>(event), rx));
                }
                Err(broadcast::error::RecvError::Lagged(_)) => {}
                Err(broadcast::error::RecvError::Closed) => return None,
            }
        }
    });
    Sse::new(stream).keep_alive(KeepAlive::default()).into_response()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn member(name: &str) -> RoomMember {
        RoomMember {
            username: name.into(),
            status: UserStatus::Online,
            avg_speed: 0,
            files: 1,
            folders: 1,
            slots_full: false,
            country: None,
        }
    }

    #[test]
    fn keeps_private_history_and_unread_counts() {
        let chat = Chat::default();
        let mut updates = chat.updates.subscribe();
        chat.apply(ChatEvent::PrivateMessage { timestamp: 10, username: "alice".into(), message: "hi".into() });
        chat.sent("me", "alice", "hello");
        let overview = chat.overview();
        assert_eq!(overview.conversations[0].unread, 1);
        assert_eq!(overview.conversations[0].last.as_ref().unwrap().text, "hello");
        assert!(matches!(updates.try_recv().unwrap(), ChatUpdate::Conversation { .. }));
    }

    #[test]
    fn tracks_room_members_and_lines() {
        let chat = Chat::default();
        chat.lock().joined.push("ambient".into());
        chat.apply(ChatEvent::JoinedRoom { room: "ambient".into(), members: vec![member("bob")] });
        chat.apply(ChatEvent::UserJoinedRoom { room: "ambient".into(), member: member("carol") });
        chat.apply(ChatEvent::UserLeftRoom { room: "ambient".into(), username: "bob".into() });
        chat.apply(ChatEvent::RoomMessage { room: "ambient".into(), username: "carol".into(), message: "hey".into() });
        chat.apply(ChatEvent::RoomList(vec![delune_soulseek::RoomSummary { name: "jazz".into(), users: 9 }]));

        let state = chat.lock();
        let room = &state.rooms["ambient"];
        assert_eq!(room.members.keys().collect::<Vec<_>>(), ["carol"]);
        assert_eq!(room.messages.len(), 1);
        drop(state);
        let rooms = chat.overview().rooms;
        assert_eq!((rooms[0].name.as_str(), rooms[0].joined, rooms[0].unread), ("ambient", true, 1));
        assert_eq!((rooms[1].name.as_str(), rooms[1].joined), ("jazz", false));
    }

    #[test]
    fn room_names_follow_soulseek_rules() {
        assert!(valid_room("ambient"));
        assert!(!valid_room(" ambient"));
        assert!(!valid_room("two  spaces"));
        assert!(!valid_room("ümlaut"));
        assert!(!valid_room(&"x".repeat(25)));
    }
}
