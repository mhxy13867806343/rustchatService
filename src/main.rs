use futures_util::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::net::{TcpListener, TcpStream};
use tokio::signal;
use tokio::sync::{broadcast, RwLock};
use tokio_tungstenite::{accept_async, tungstenite::Message};
use tracing::{error, info, warn};
mod errors;
mod db;
mod rate_limit;
mod comments;
use uuid::Uuid;
use axum::{routing::{get, post, delete}, Router, Json, extract::{Path, State, Query}};
use axum::http::{HeaderMap, StatusCode};
use jsonwebtoken::{encode, decode, Header as JwtHeader, EncodingKey, DecodingKey, Validation};
use utoipa::{OpenApi, ToSchema};
use utoipa_swagger_ui::SwaggerUi;
use hmac::{Hmac, Mac};
use sha2::Sha256;
use hex;

// 消息类型定义
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum ChatMessage {
    #[serde(rename = "join")]
    Join { username: String, room_id: String },
    #[serde(rename = "leave")]
    Leave { username: String, room_id: String },
    #[serde(rename = "message")]
    Message {
        username: String,
        room_id: String,
        content: String,
        timestamp: u64,
    },
    #[serde(rename = "user_joined")]
    UserJoined { username: String, room_id: String },
    #[serde(rename = "user_left")]
    UserLeft { username: String, room_id: String },
    #[serde(rename = "error")]
    Error { message: String },
}

// 用户连接信息
#[derive(Debug, Clone)]
pub struct User {
    pub id: Uuid,
    pub username: String,
    pub room_id: String,
    pub sender: broadcast::Sender<ChatMessage>,
}

// 聊天服务器状态
#[derive(Debug)]
pub struct ChatServer {
    // 房间ID -> 用户列表
    pub rooms: Arc<RwLock<HashMap<String, HashMap<Uuid, User>>>>,
    // 全局消息广播器
    pub global_sender: broadcast::Sender<ChatMessage>,
    // 社交关系：actor_key(用户名或uid_hash) -> 目标用户名集合
    pub follows: Arc<RwLock<HashMap<String, HashSet<String>>>>,
    pub blocks: Arc<RwLock<HashMap<String, HashSet<String>>>>,
    pub mutes: Arc<RwLock<HashMap<String, HashSet<String>>>>,
}

impl ChatServer {
    pub fn new() -> Self {
        let (global_sender, _) = broadcast::channel(1000);
        Self {
            rooms: Arc::new(RwLock::new(HashMap::new())),
            global_sender,
            follows: Arc::new(RwLock::new(HashMap::new())),
            blocks: Arc::new(RwLock::new(HashMap::new())),
            mutes: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    // 用户加入房间
    pub async fn join_room(&self, user: User) {
        let mut rooms = self.rooms.write().await;
        let room = rooms.entry(user.room_id.clone()).or_insert_with(HashMap::new);
        
        // 通知房间内其他用户有新用户加入
        let join_message = ChatMessage::UserJoined {
            username: user.username.clone(),
            room_id: user.room_id.clone(),
        };
        
        // 向房间内所有用户广播加入消息
        for existing_user in room.values() {
            let _ = existing_user.sender.send(join_message.clone());
        }
        
        room.insert(user.id, user);
        info!("User joined room");
    }

    // 用户离开房间
    pub async fn leave_room(&self, user_id: Uuid, room_id: &str) {
        let mut rooms = self.rooms.write().await;
        if let Some(room) = rooms.get_mut(room_id) {
            if let Some(user) = room.remove(&user_id) {
                // 通知房间内其他用户有用户离开
                let leave_message = ChatMessage::UserLeft {
                    username: user.username,
                    room_id: room_id.to_string(),
                };
                
                // 向房间内剩余用户广播离开消息
                for remaining_user in room.values() {
                    let _ = remaining_user.sender.send(leave_message.clone());
                }
                
                // 如果房间为空，删除房间
                if room.is_empty() {
                    rooms.remove(room_id);
                }
                info!("User left room");
            }
        }
    }

    // 广播消息到房间
    pub async fn broadcast_to_room(&self, room_id: &str, message: ChatMessage) {
        let rooms = self.rooms.read().await;
        if let Some(room) = rooms.get(room_id) {
            for user in room.values() {
                let _ = user.sender.send(message.clone());
            }
        }
    }

    // 获取房间用户列表
    pub async fn get_room_users(&self, room_id: &str) -> Vec<String> {
        let rooms = self.rooms.read().await;
        if let Some(room) = rooms.get(room_id) {
            room.values().map(|user| user.username.clone()).collect()
        } else {
            Vec::new()
        }
    }

    pub async fn follow(&self, actor_key: &str, target_username: &str) -> bool {
        let mut map = self.follows.write().await;
        let set = map.entry(actor_key.to_string()).or_insert_with(HashSet::new);
        set.insert(target_username.to_string())
    }

    pub async fn unfollow(&self, actor_key: &str, target_username: &str) -> bool {
        let mut map = self.follows.write().await;
        if let Some(set) = map.get_mut(actor_key) {
            set.remove(target_username)
        } else { false }
    }

    pub async fn block(&self, actor_key: &str, target_username: &str) -> bool {
        let mut map = self.blocks.write().await;
        let set = map.entry(actor_key.to_string()).or_insert_with(HashSet::new);
        set.insert(target_username.to_string())
    }

    pub async fn unblock(&self, actor_key: &str, target_username: &str) -> bool {
        let mut map = self.blocks.write().await;
        if let Some(set) = map.get_mut(actor_key) {
            set.remove(target_username)
        } else { false }
    }

    pub async fn mute(&self, actor_key: &str, target_username: &str) -> bool {
        let mut map = self.mutes.write().await;
        let set = map.entry(actor_key.to_string()).or_insert_with(HashSet::new);
        set.insert(target_username.to_string())
    }

    pub async fn unmute(&self, actor_key: &str, target_username: &str) -> bool {
        let mut map = self.mutes.write().await;
        if let Some(set) = map.get_mut(actor_key) {
            set.remove(target_username)
        } else { false }
    }

    pub async fn search_room_users(&self, room_id: &str, query: &str, actor_key: Option<&str>) -> Vec<String> {
        let users = self.get_room_users(room_id).await;
        let q = query.trim();
        let (mention, kw) = if q.starts_with('@') { (true, q.trim_start_matches('@')) } else { (false, q) };
        let kw_lower = kw.to_lowercase();
        let mut filtered: Vec<String> = users
            .into_iter()
            .filter(|u| {
                let lu = u.to_lowercase();
                if mention { lu.starts_with(&kw_lower) } else { lu.contains(&kw_lower) }
            })
            .collect();
        if let Some(actor) = actor_key {
            let blocks = self.blocks.read().await;
            let mutes = self.mutes.read().await;
            filtered.retain(|u| {
                !blocks.get(actor).map_or(false, |s| s.contains(u)) &&
                !mutes.get(actor).map_or(false, |s| s.contains(u))
            });
        }
        filtered
    }
}

// 健康检查响应与文档
#[derive(serde::Serialize, ToSchema)]
struct HealthResponse {
    status: String,
}

#[derive(serde::Serialize, ToSchema)]
struct ApiErrorEnvelope {
    code: i32,
    message: String,
}

#[derive(serde::Serialize, ToSchema)]
struct HealthEnvelope {
    code: i32,
    message: String,
    data: HealthResponse,
}

#[utoipa::path(
    get,
    path = "/health",
    responses(
        (status = 200, description = "OK", body = HealthEnvelope)
    )
)]
async fn health_handler() -> Json<HealthEnvelope> {
    Json(HealthEnvelope { code: 0, message: "ok".into(), data: HealthResponse { status: "ok".into() } })
}

#[derive(serde::Serialize, serde::Deserialize, ToSchema)]
struct PublishRequest {
    username: String,
    content: String,
}

#[derive(serde::Deserialize, ToSchema)]
struct AuthQueryPublish {
    ts: u64,
    nonce: String,
    uid_hash: String,
    sig: String,
}

#[derive(serde::Serialize, ToSchema)]
struct PublishEnvelope {
    code: i32,
    message: String,
}

#[utoipa::path(
    post,
    path = "/api/rooms/{room_id}/publish",
    request_body = PublishRequest,
    params(
        ("room_id" = String, Path, description = "目标房间 ID"),
        ("ts" = u64, Query, description = "时间戳（签名参与）"),
        ("nonce" = String, Query, description = "随机数（签名参与）"),
        ("uid_hash" = String, Query, description = "用户唯一哈希，36 位字母数字（签名参与）"),
        ("sig" = String, Query, description = "HMAC-SHA256 十六进制签名")
    ),
    responses(
        (status = 200, description = "消息已广播", body = PublishEnvelope),
        (status = 401, description = "未授权", body = ApiErrorEnvelope)
    )
)]
async fn publish_handler(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(room_id): Path<String>,
    Query(auth): Query<AuthQueryPublish>,
    Json(req): Json<PublishRequest>,
) -> Result<Json<PublishEnvelope>, (StatusCode, Json<ApiErrorEnvelope>)> {
    let canonical = format!(
        "room_id={}&username={}&content={}&ts={}&nonce={}&uid_hash={}",
        room_id, req.username, req.content, auth.ts, auth.nonce, auth.uid_hash
    );
    
    if !verify_auth(&headers, &auth.ts, &auth.nonce, &auth.uid_hash, &auth.sig, &canonical).await {
        return Err((StatusCode::UNAUTHORIZED, Json(ApiErrorEnvelope { code: 401, message: "invalid auth".into() })));
    }

    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    state.chat_server
        .broadcast_to_room(&room_id, ChatMessage::Message {
            username: req.username,
            room_id: room_id.clone(),
            content: req.content,
            timestamp: ts,
        })
        .await;
    Ok(Json(PublishEnvelope { code: 0, message: "ok".into() }))
}

#[derive(serde::Serialize, ToSchema)]
struct UsersEnvelope {
    code: i32,
    message: String,
    data: Vec<String>,
}

#[derive(serde::Deserialize, ToSchema)]
struct AuthQueryUsers {
    ts: u64,
    nonce: String,
    uid_hash: String,
    sig: String,
}

#[utoipa::path(
    get,
    path = "/api/rooms/{room_id}/users",
    params(
        ("room_id" = String, Path, description = "房间 ID"),
        ("ts" = u64, Query, description = "时间戳（签名参与）"),
        ("nonce" = String, Query, description = "随机数（签名参与）"),
        ("uid_hash" = String, Query, description = "用户唯一哈希，36 位字母数字（签名参与）"),
        ("sig" = String, Query, description = "HMAC-SHA256 十六进制签名")
    ),
    responses(
        (status = 200, description = "房间用户列表", body = UsersEnvelope),
        (status = 401, description = "未授权", body = ApiErrorEnvelope)
    )
)]
async fn list_users_handler(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(room_id): Path<String>,
    Query(auth): Query<AuthQueryUsers>,
) -> Result<Json<UsersEnvelope>, (StatusCode, Json<ApiErrorEnvelope>)> {
    let canonical = format!("room_id={}&ts={}&nonce={}&uid_hash={}", room_id, auth.ts, auth.nonce, auth.uid_hash);
    
    if !verify_auth(&headers, &auth.ts, &auth.nonce, &auth.uid_hash, &auth.sig, &canonical).await {
        return Err((StatusCode::UNAUTHORIZED, Json(ApiErrorEnvelope { code: 401, message: "invalid auth".into() })));
    }

    let users = state.chat_server.get_room_users(&room_id).await;
    Ok(Json(UsersEnvelope { code: 0, message: "ok".into(), data: users }))
}

#[derive(serde::Deserialize, ToSchema)]
struct SearchQuery {
    #[serde(rename = "q")]
    q: String,
    ts: u64,
    nonce: String,
    uid_hash: String,
    sig: String,
}

#[derive(serde::Serialize, ToSchema)]
struct SearchEnvelope {
    code: i32,
    message: String,
    data: Vec<String>,
}

#[utoipa::path(
    get,
    path = "/api/rooms/{room_id}/search",
    params(
        ("room_id" = String, Path, description = "房间 ID"),
        ("q" = String, Query, description = "搜索关键字，@ 前缀按开头匹配"),
        ("ts" = u64, Query, description = "时间戳（签名参与）"),
        ("nonce" = String, Query, description = "随机数（签名参与）"),
        ("uid_hash" = String, Query, description = "用户唯一哈希，36 位字母数字（签名参与）"),
        ("sig" = String, Query, description = "HMAC-SHA256 十六进制签名")
    ),
    responses(
        (status = 200, description = "搜索结果", body = SearchEnvelope),
        (status = 401, description = "未授权", body = ApiErrorEnvelope)
    )
)]
async fn search_users_handler(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(room_id): Path<String>,
    Query(params): Query<SearchQuery>,
) -> Result<Json<SearchEnvelope>, (StatusCode, Json<ApiErrorEnvelope>)> {
    let canonical = format!("room_id={}&q={}&ts={}&nonce={}&uid_hash={}", room_id, params.q, params.ts, params.nonce, params.uid_hash);
    
    let mut actor_key: Option<String> = None;
    
    // 检查 JWT
    if let Some(auth_header) = headers.get(axum::http::header::AUTHORIZATION).and_then(|v| v.to_str().ok()) {
        if let Some(token) = auth_header.strip_prefix("Bearer ") {
            let secret = std::env::var("JWT_SECRET").unwrap_or_else(|_| "dev-secret".to_string());
            let validation = Validation::default();
            if let Ok(tok) = decode::<Claims>(token, &DecodingKey::from_secret(secret.as_bytes()), &validation) {
                actor_key = Some(tok.claims.sub);
            }
        }
    }
    
    if actor_key.is_none() {
        if !verify_auth(&headers, &params.ts, &params.nonce, &params.uid_hash, &params.sig, &canonical).await {
            return Err((StatusCode::UNAUTHORIZED, Json(ApiErrorEnvelope { code: 401, message: "invalid auth".into() })));
        }
        actor_key = Some(params.uid_hash.clone());
    }
    
    let results = state.chat_server.search_room_users(&room_id, &params.q, actor_key.as_deref()).await;
    Ok(Json(SearchEnvelope { code: 0, message: "ok".into(), data: results }))
}

#[derive(serde::Serialize, serde::Deserialize, ToSchema)]
struct SocialActionRequest {
    action: String, // follow | unfollow | block | unblock | mute | unmute
    target: String, // 目标用户名
    room_id: Option<String>,
}

#[derive(serde::Deserialize, ToSchema)]
struct AuthQueryAction {
    ts: u64,
    nonce: String,
    uid_hash: String,
    sig: String,
}

#[derive(serde::Serialize, ToSchema)]
struct SocialActionEnvelope {
    code: i32,
    message: String,
}

#[utoipa::path(
    post,
    path = "/api/social/action",
    request_body = SocialActionRequest,
    params(
        ("ts" = u64, Query, description = "时间戳（签名参与）"),
        ("nonce" = String, Query, description = "随机数（签名参与）"),
        ("uid_hash" = String, Query, description = "用户唯一哈希，36 位字母数字（签名参与）"),
        ("sig" = String, Query, description = "HMAC-SHA256 十六进制签名")
    ),
    responses(
        (status = 200, description = "操作结果", body = SocialActionEnvelope),
        (status = 401, description = "未授权", body = ApiErrorEnvelope)
    )
)]
async fn social_action_handler(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(auth): Query<AuthQueryAction>,
    Json(req): Json<SocialActionRequest>,
) -> Result<Json<SocialActionEnvelope>, (StatusCode, Json<ApiErrorEnvelope>)> {
    let canonical = format!("action={}&target={}&ts={}&nonce={}&uid_hash={}", req.action, req.target, auth.ts, auth.nonce, auth.uid_hash);
    
    let mut actor_key: Option<String> = None;
    
    // 检查 JWT
    if let Some(auth_header) = headers.get(axum::http::header::AUTHORIZATION).and_then(|v| v.to_str().ok()) {
        if let Some(token) = auth_header.strip_prefix("Bearer ") {
            let secret = std::env::var("JWT_SECRET").unwrap_or_else(|_| "dev-secret".to_string());
            let validation = Validation::default();
            if let Ok(tok) = decode::<Claims>(token, &DecodingKey::from_secret(secret.as_bytes()), &validation) {
                actor_key = Some(tok.claims.sub);
            }
        }
    }
    
    if actor_key.is_none() {
        if !verify_auth(&headers, &auth.ts, &auth.nonce, &auth.uid_hash, &auth.sig, &canonical).await {
            return Err((StatusCode::UNAUTHORIZED, Json(ApiErrorEnvelope { code: 401, message: "invalid auth".into() })));
        }
        actor_key = Some(auth.uid_hash.clone());
    }
    
    let actor = actor_key.unwrap_or_default();
    let changed = match req.action.as_str() {
        "follow" => state.chat_server.follow(&actor, &req.target).await,
        "unfollow" => state.chat_server.unfollow(&actor, &req.target).await,
        "block" => state.chat_server.block(&actor, &req.target).await,
        "unblock" => state.chat_server.unblock(&actor, &req.target).await,
        "mute" => state.chat_server.mute(&actor, &req.target).await,
        "unmute" => state.chat_server.unmute(&actor, &req.target).await,
        _ => return Err((StatusCode::BAD_REQUEST, Json(ApiErrorEnvelope { code: 400, message: "invalid action".into() }))),
    };
    let message = if changed { "ok" } else { "no change" };
    Ok(Json(SocialActionEnvelope { code: 0, message: message.into() }))
}

#[derive(serde::Serialize, serde::Deserialize, ToSchema)]
struct LoginRequest {
    username: String,
    password: String,
}

#[derive(serde::Serialize, serde::Deserialize, ToSchema)]
struct LoginResponse {
    token: String,
}

#[derive(serde::Serialize, serde::Deserialize)]
struct Claims {
    sub: String,
    exp: usize,
}

#[derive(serde::Serialize, ToSchema)]
struct LoginEnvelope {
    code: i32,
    message: String,
    data: LoginResponse,
}

#[utoipa::path(
    post,
    path = "/auth/login",
    request_body = LoginRequest,
    responses(
        (status = 200, description = "登录成功，返回 JWT", body = LoginEnvelope),
        (status = 401, description = "登录失败", body = ApiErrorEnvelope)
    )
)]
async fn login_handler(Json(req): Json<LoginRequest>) -> Result<Json<LoginEnvelope>, (StatusCode, Json<ApiErrorEnvelope>)> {
    let expect_user = std::env::var("DEMO_USER").unwrap_or_else(|_| "py-bot".to_string());
    let expect_pass = std::env::var("DEMO_PASS").unwrap_or_else(|_| "password".to_string());
    if req.username != expect_user || req.password != expect_pass {
        return Err((StatusCode::UNAUTHORIZED, Json(ApiErrorEnvelope { code: 401, message: "invalid credentials".into() })));
    }
    let secret = std::env::var("JWT_SECRET").unwrap_or_else(|_| "dev-secret".to_string());
    let exp = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_secs() + 24 * 3600;
    let claims = Claims { sub: req.username, exp: exp as usize };
    let token = encode(&JwtHeader::default(), &claims, &EncodingKey::from_secret(secret.as_bytes())).map_err(|_| (StatusCode::INTERNAL_SERVER_ERROR, Json(ApiErrorEnvelope { code: 500, message: "token encode failed".into() })))?;
    Ok(Json(LoginEnvelope { code: 0, message: "ok".into(), data: LoginResponse { token } }))
}

fn secure_eq(a: &str, b: &str) -> bool {
    if a.len() != b.len() { return false; }
    let mut diff = 0u8;
    for (x, y) in a.as_bytes().iter().zip(b.as_bytes().iter()) {
        diff |= x ^ y;
    }
    diff == 0
}

// ==================== 评论相关的结构体和处理器 ====================

#[derive(serde::Serialize, serde::Deserialize, ToSchema)]
struct CreateCommentRequest {
    post_id: i64,
    author_id: i64,
    parent_comment_id: Option<i64>,
    content: String,
    at_user_id: Option<i64>,
    idempotency_key: String,
}

#[derive(serde::Serialize, ToSchema, Clone)]
struct CommentResponse {
    id: i64,
    post_id: i64,
    author_id: i64,
    parent_comment_id: Option<i64>,
    content: String,
    at_user_id: Option<i64>,
    created_at: String,
}

#[derive(serde::Serialize, ToSchema)]
struct CommentWithReplies {
    id: i64,
    post_id: i64,
    author_id: i64,
    content: String,
    at_user_id: Option<i64>,
    created_at: String,
    replies: Vec<CommentReply>,
}

#[derive(serde::Serialize, ToSchema)]
struct CommentReply {
    id: i64,
    author_id: i64,
    content: String,
    at_user_id: Option<i64>,
    created_at: String,
}

#[derive(serde::Serialize, ToSchema)]
struct CommentEnvelope {
    code: i32,
    message: String,
    data: Option<CommentResponse>,
}

#[derive(serde::Serialize, ToSchema)]
struct CommentsListEnvelope {
    code: i32,
    message: String,
    data: Vec<CommentWithReplies>,
}

#[derive(serde::Serialize, ToSchema)]
struct PostStatusResponse {
    exists: bool,
    deleted: bool,
    locked: bool,
    message: String,
}

#[derive(serde::Serialize, ToSchema)]
struct PostStatusEnvelope {
    code: i32,
    message: String,
    data: PostStatusResponse,
}

#[derive(serde::Deserialize, ToSchema)]
struct AuthQueryComment {
    ts: u64,
    nonce: String,
    uid_hash: String,
    sig: String,
}

// 共享的应用状态
#[derive(Clone)]
struct AppState {
    chat_server: Arc<ChatServer>,
    comment_service: Option<Arc<comments::CommentService>>,
}

#[utoipa::path(
    post,
    path = "/api/comments",
    request_body = CreateCommentRequest,
    params(
        ("ts" = u64, Query, description = "时间戳（签名参与）"),
        ("nonce" = String, Query, description = "随机数（签名参与）"),
        ("uid_hash" = String, Query, description = "用户唯一哈希，36 位字母数字（签名参与）"),
        ("sig" = String, Query, description = "HMAC-SHA256 十六进制签名")
    ),
    responses(
        (status = 200, description = "评论创建成功", body = CommentEnvelope),
        (status = 401, description = "未授权", body = ApiErrorEnvelope),
        (status = 429, description = "请求过于频繁", body = ApiErrorEnvelope)
    )
)]
async fn create_comment_handler(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(auth): Query<AuthQueryComment>,
    Json(req): Json<CreateCommentRequest>,
) -> Result<Json<CommentEnvelope>, (StatusCode, Json<ApiErrorEnvelope>)> {
    // 验证认证
    if !verify_auth(&headers, &auth.ts, &auth.nonce, &auth.uid_hash, &auth.sig, &format!("post_id={}&author_id={}&content={}&ts={}&nonce={}&uid_hash={}", req.post_id, req.author_id, req.content, auth.ts, auth.nonce, auth.uid_hash)).await {
        return Err((StatusCode::UNAUTHORIZED, Json(ApiErrorEnvelope { code: 401, message: "invalid auth".into() })));
    }

    let service = state.comment_service.as_ref().ok_or_else(|| {
        (StatusCode::SERVICE_UNAVAILABLE, Json(ApiErrorEnvelope { code: 503, message: "comment service not available".into() }))
    })?;

    // 获取客户端 IP（简化处理）
    let ip_key = headers
        .get("x-forwarded-for")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("unknown")
        .to_string();

    let input = comments::CreateCommentInput {
        post_id: req.post_id,
        author_id: req.author_id,
        parent_comment_id: req.parent_comment_id,
        content: req.content,
        at_user_id: req.at_user_id,
        idempotency_key: req.idempotency_key,
        ip_key,
    };

    match service.create_comment(input).await {
        Ok(comment) => {
            let resp = CommentResponse {
                id: comment.id,
                post_id: comment.post_id,
                author_id: comment.author_id,
                parent_comment_id: comment.parent_comment_id,
                content: comment.content,
                at_user_id: comment.at_user_id,
                created_at: comment.created_at.to_rfc3339(),
            };
            Ok(Json(CommentEnvelope { code: 0, message: "评论创建成功".into(), data: Some(resp) }))
        }
        Err(e) => {
            let status = match e.code() {
                404 => StatusCode::NOT_FOUND,
                410 => StatusCode::GONE,
                423 => StatusCode::LOCKED,
                429 => StatusCode::TOO_MANY_REQUESTS,
                422 => StatusCode::UNPROCESSABLE_ENTITY,
                408 => StatusCode::REQUEST_TIMEOUT,
                _ => StatusCode::INTERNAL_SERVER_ERROR,
            };
            Err((status, Json(ApiErrorEnvelope { code: e.code() as i32, message: e.to_string() })))
        }
    }
}

#[utoipa::path(
    get,
    path = "/api/posts/{post_id}/status",
    params(
        ("post_id" = i64, Path, description = "帖子 ID"),
        ("ts" = u64, Query, description = "时间戳（签名参与）"),
        ("nonce" = String, Query, description = "随机数（签名参与）"),
        ("uid_hash" = String, Query, description = "用户唯一哈希，36 位字母数字（签名参与）"),
        ("sig" = String, Query, description = "HMAC-SHA256 十六进制签名")
    ),
    responses(
        (status = 200, description = "帖子状态", body = PostStatusEnvelope),
        (status = 401, description = "未授权", body = ApiErrorEnvelope)
    )
)]
async fn check_post_status_handler(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(post_id): Path<i64>,
    Query(auth): Query<AuthQueryComment>,
) -> Result<Json<PostStatusEnvelope>, (StatusCode, Json<ApiErrorEnvelope>)> {
    // 验证认证
    if !verify_auth(&headers, &auth.ts, &auth.nonce, &auth.uid_hash, &auth.sig, &format!("post_id={}&ts={}&nonce={}&uid_hash={}", post_id, auth.ts, auth.nonce, auth.uid_hash)).await {
        return Err((StatusCode::UNAUTHORIZED, Json(ApiErrorEnvelope { code: 401, message: "invalid auth".into() })));
    }

    let service = state.comment_service.as_ref().ok_or_else(|| {
        (StatusCode::SERVICE_UNAVAILABLE, Json(ApiErrorEnvelope { code: 503, message: "comment service not available".into() }))
    })?;

    match service.check_post_status(post_id).await {
        Ok(status) => {
            let (locked, message) = match status {
                comments::PostStatus::Active => (false, "帖子正常".to_string()),
                comments::PostStatus::Locked => (true, "帖子已锁定，无法评论".to_string()),
            };
            
            let response = PostStatusResponse {
                exists: true,
                deleted: false,
                locked,
                message,
            };
            
            Ok(Json(PostStatusEnvelope { code: 0, message: "ok".into(), data: response }))
        }
        Err(e) => {
            match e.code() {
                404 => {
                    // 帖子不存在
                    let response = PostStatusResponse {
                        exists: false,
                        deleted: false,
                        locked: false,
                        message: "帖子不存在".to_string(),
                    };
                    Ok(Json(PostStatusEnvelope { code: 404, message: "帖子不存在".into(), data: response }))
                }
                410 => {
                    // 帖子已删除
                    let response = PostStatusResponse {
                        exists: true,
                        deleted: true,
                        locked: false,
                        message: "帖子已被删除".to_string(),
                    };
                    Ok(Json(PostStatusEnvelope { code: 410, message: "帖子已被删除".into(), data: response }))
                }
                _ => {
                    Err((StatusCode::INTERNAL_SERVER_ERROR, Json(ApiErrorEnvelope { code: e.code() as i32, message: e.to_string() })))
                }
            }
        }
    }
}

#[utoipa::path(
    get,
    path = "/api/posts/{post_id}/comments",
    params(
        ("post_id" = i64, Path, description = "帖子 ID"),
        ("ts" = u64, Query, description = "时间戳（签名参与）"),
        ("nonce" = String, Query, description = "随机数（签名参与）"),
        ("uid_hash" = String, Query, description = "用户唯一哈希，36 位字母数字（签名参与）"),
        ("sig" = String, Query, description = "HMAC-SHA256 十六进制签名")
    ),
    responses(
        (status = 200, description = "评论列表（嵌套结构）", body = CommentsListEnvelope),
        (status = 401, description = "未授权", body = ApiErrorEnvelope),
        (status = 404, description = "帖子不存在", body = ApiErrorEnvelope),
        (status = 410, description = "帖子已删除", body = ApiErrorEnvelope)
    )
)]
async fn get_comments_handler(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(post_id): Path<i64>,
    Query(auth): Query<AuthQueryComment>,
) -> Result<Json<CommentsListEnvelope>, (StatusCode, Json<ApiErrorEnvelope>)> {
    // 验证认证
    if !verify_auth(&headers, &auth.ts, &auth.nonce, &auth.uid_hash, &auth.sig, &format!("post_id={}&ts={}&nonce={}&uid_hash={}", post_id, auth.ts, auth.nonce, auth.uid_hash)).await {
        return Err((StatusCode::UNAUTHORIZED, Json(ApiErrorEnvelope { code: 401, message: "invalid auth".into() })));
    }

    let service = state.comment_service.as_ref().ok_or_else(|| {
        (StatusCode::SERVICE_UNAVAILABLE, Json(ApiErrorEnvelope { code: 503, message: "comment service not available".into() }))
    })?;

    // 先检查帖子状态
    match service.check_post_status(post_id).await {
        Ok(_) => {
            // 帖子存在且正常，继续获取评论
        }
        Err(e) => {
            let status = match e.code() {
                404 => StatusCode::NOT_FOUND,
                410 => StatusCode::GONE,
                _ => StatusCode::INTERNAL_SERVER_ERROR,
            };
            return Err((status, Json(ApiErrorEnvelope { code: e.code() as i32, message: e.to_string() })));
        }
    }

    match service.get_comments_tree(post_id).await {
        Ok(tree) => {
            let nested_comments: Vec<CommentWithReplies> = tree.into_iter().map(|(parent, replies)| {
                CommentWithReplies {
                    id: parent.id,
                    post_id: parent.post_id,
                    author_id: parent.author_id,
                    content: parent.content,
                    at_user_id: parent.at_user_id,
                    created_at: parent.created_at.to_rfc3339(),
                    replies: replies.into_iter().map(|r| CommentReply {
                        id: r.id,
                        author_id: r.author_id,
                        content: r.content,
                        at_user_id: r.at_user_id,
                        created_at: r.created_at.to_rfc3339(),
                    }).collect(),
                }
            }).collect();
            
            Ok(Json(CommentsListEnvelope { code: 0, message: "ok".into(), data: nested_comments }))
        }
        Err(e) => {
            let status = match e.code() {
                404 => StatusCode::NOT_FOUND,
                410 => StatusCode::GONE,
                _ => StatusCode::INTERNAL_SERVER_ERROR,
            };
            Err((status, Json(ApiErrorEnvelope { code: e.code() as i32, message: e.to_string() })))
        }
    }
}

#[utoipa::path(
    delete,
    path = "/api/posts/{post_id}",
    params(
        ("post_id" = i64, Path, description = "帖子 ID"),
        ("ts" = u64, Query, description = "时间戳（签名参与）"),
        ("nonce" = String, Query, description = "随机数（签名参与）"),
        ("uid_hash" = String, Query, description = "用户唯一哈希，36 位字母数字（签名参与）"),
        ("sig" = String, Query, description = "HMAC-SHA256 十六进制签名")
    ),
    responses(
        (status = 200, description = "删除成功", body = CommentEnvelope),
        (status = 401, description = "未授权", body = ApiErrorEnvelope),
        (status = 404, description = "帖子不存在", body = ApiErrorEnvelope),
        (status = 410, description = "帖子已删除", body = ApiErrorEnvelope)
    )
)]
async fn delete_post_handler(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(post_id): Path<i64>,
    Query(auth): Query<AuthQueryComment>,
) -> Result<Json<CommentEnvelope>, (StatusCode, Json<ApiErrorEnvelope>)> {
    // 验证认证
    if !verify_auth(&headers, &auth.ts, &auth.nonce, &auth.uid_hash, &auth.sig, &format!("post_id={}&ts={}&nonce={}&uid_hash={}", post_id, auth.ts, auth.nonce, auth.uid_hash)).await {
        return Err((StatusCode::UNAUTHORIZED, Json(ApiErrorEnvelope { code: 401, message: "invalid auth".into() })));
    }

    let service = state.comment_service.as_ref().ok_or_else(|| {
        (StatusCode::SERVICE_UNAVAILABLE, Json(ApiErrorEnvelope { code: 503, message: "comment service not available".into() }))
    })?;

    match service.delete_post_soft(post_id, 0).await {
        Ok(_) => Ok(Json(CommentEnvelope { code: 0, message: "帖子及其所有评论已删除".into(), data: None })),
        Err(e) => {
            let status = match e.code() {
                404 => StatusCode::NOT_FOUND,
                410 => StatusCode::GONE,
                _ => StatusCode::INTERNAL_SERVER_ERROR,
            };
            Err((status, Json(ApiErrorEnvelope { code: e.code() as i32, message: e.to_string() })))
        }
    }
}

#[utoipa::path(
    delete,
    path = "/api/comments/{comment_id}",
    params(
        ("comment_id" = i64, Path, description = "评论 ID"),
        ("ts" = u64, Query, description = "时间戳（签名参与）"),
        ("nonce" = String, Query, description = "随机数（签名参与）"),
        ("uid_hash" = String, Query, description = "用户唯一哈希，36 位字母数字（签名参与）"),
        ("sig" = String, Query, description = "HMAC-SHA256 十六进制签名")
    ),
    responses(
        (status = 200, description = "删除成功", body = CommentEnvelope),
        (status = 401, description = "未授权", body = ApiErrorEnvelope),
        (status = 404, description = "评论不存在", body = ApiErrorEnvelope),
        (status = 410, description = "评论已删除", body = ApiErrorEnvelope)
    )
)]
async fn delete_comment_handler(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(comment_id): Path<i64>,
    Query(auth): Query<AuthQueryComment>,
) -> Result<Json<CommentEnvelope>, (StatusCode, Json<ApiErrorEnvelope>)> {
    // 验证认证
    if !verify_auth(&headers, &auth.ts, &auth.nonce, &auth.uid_hash, &auth.sig, &format!("comment_id={}&ts={}&nonce={}&uid_hash={}", comment_id, auth.ts, auth.nonce, auth.uid_hash)).await {
        return Err((StatusCode::UNAUTHORIZED, Json(ApiErrorEnvelope { code: 401, message: "invalid auth".into() })));
    }

    let service = state.comment_service.as_ref().ok_or_else(|| {
        (StatusCode::SERVICE_UNAVAILABLE, Json(ApiErrorEnvelope { code: 503, message: "comment service not available".into() }))
    })?;

    match service.delete_comment_soft(comment_id, 0).await {
        Ok(_) => Ok(Json(CommentEnvelope { code: 0, message: "评论已删除（如果是一级评论，其下的所有回复也已删除）".into(), data: None })),
        Err(e) => {
            let status = match e.code() {
                404 => StatusCode::NOT_FOUND,
                410 => StatusCode::GONE,
                _ => StatusCode::INTERNAL_SERVER_ERROR,
            };
            Err((status, Json(ApiErrorEnvelope { code: e.code() as i32, message: e.to_string() })))
        }
    }
}

#[derive(serde::Serialize, serde::Deserialize, ToSchema)]
struct ReactRequest {
    resource_type: i16, // 1=post, 2=comment
    resource_id: i64,
    reactor_id: i64,
    reaction_type: i16, // 1=like, 2=favorite
    idempotency_key: String,
}

#[utoipa::path(
    post,
    path = "/api/reactions",
    request_body = ReactRequest,
    params(
        ("ts" = u64, Query, description = "时间戳（签名参与）"),
        ("nonce" = String, Query, description = "随机数（签名参与）"),
        ("uid_hash" = String, Query, description = "用户唯一哈希，36 位字母数字（签名参与）"),
        ("sig" = String, Query, description = "HMAC-SHA256 十六进制签名")
    ),
    responses(
        (status = 200, description = "反应添加成功", body = CommentEnvelope),
        (status = 401, description = "未授权", body = ApiErrorEnvelope)
    )
)]
async fn react_handler(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(auth): Query<AuthQueryComment>,
    Json(req): Json<ReactRequest>,
) -> Result<Json<CommentEnvelope>, (StatusCode, Json<ApiErrorEnvelope>)> {
    // 验证认证
    if !verify_auth(&headers, &auth.ts, &auth.nonce, &auth.uid_hash, &auth.sig, &format!("resource_type={}&resource_id={}&reactor_id={}&reaction_type={}&ts={}&nonce={}&uid_hash={}", req.resource_type, req.resource_id, req.reactor_id, req.reaction_type, auth.ts, auth.nonce, auth.uid_hash)).await {
        return Err((StatusCode::UNAUTHORIZED, Json(ApiErrorEnvelope { code: 401, message: "invalid auth".into() })));
    }

    let service = state.comment_service.as_ref().ok_or_else(|| {
        (StatusCode::SERVICE_UNAVAILABLE, Json(ApiErrorEnvelope { code: 503, message: "comment service not available".into() }))
    })?;

    match service.react_idempotent(req.resource_type, req.resource_id, req.reactor_id, req.reaction_type, req.idempotency_key).await {
        Ok(_) => Ok(Json(CommentEnvelope { code: 0, message: "ok".into(), data: None })),
        Err(e) => {
            let status = match e.code() {
                _ => StatusCode::INTERNAL_SERVER_ERROR,
            };
            Err((status, Json(ApiErrorEnvelope { code: e.code() as i32, message: e.to_string() })))
        }
    }
}

// 统一的认证验证函数
async fn verify_auth(headers: &HeaderMap, ts: &u64, nonce: &str, uid_hash: &str, sig: &str, canonical: &str) -> bool {
    // 优先检查 JWT Bearer Token
    if let Some(auth_header) = headers.get(axum::http::header::AUTHORIZATION).and_then(|v| v.to_str().ok()) {
        if let Some(token) = auth_header.strip_prefix("Bearer ") {
            let secret = std::env::var("JWT_SECRET").unwrap_or_else(|_| "dev-secret".to_string());
            let validation = Validation::default();
            if decode::<Claims>(token, &DecodingKey::from_secret(secret.as_bytes()), &validation).is_ok() {
                return true;
            }
        }
    }

    // 回退到 HMAC 签名验证
    if uid_hash.len() != 36 || !uid_hash.chars().all(|c| c.is_ascii_alphanumeric()) {
        return false;
    }

    let secret = std::env::var("AUTH_SECRET").unwrap_or_else(|_| "sso-secret".to_string());
    type HmacSha256 = Hmac<Sha256>;
    let Ok(mut mac) = HmacSha256::new_from_slice(secret.as_bytes()) else { return false; };
    mac.update(canonical.as_bytes());
    let expected = hex::encode(mac.finalize().into_bytes());
    secure_eq(&expected, &sig.to_lowercase())
}

#[derive(OpenApi)]
#[openapi(
    paths(
        health_handler, 
        publish_handler, 
        list_users_handler, 
        search_users_handler, 
        social_action_handler, 
        login_handler,
        create_comment_handler,
        check_post_status_handler,
        get_comments_handler,
        delete_post_handler,
        delete_comment_handler,
        react_handler
    ),
    components(
        schemas(
            HealthResponse, HealthEnvelope, ApiErrorEnvelope, 
            PublishRequest, PublishEnvelope, 
            LoginRequest, LoginResponse, LoginEnvelope, 
            UsersEnvelope, AuthQueryPublish, AuthQueryUsers, 
            SearchQuery, SearchEnvelope, 
            SocialActionRequest, SocialActionEnvelope, AuthQueryAction,
            CreateCommentRequest, CommentResponse, CommentEnvelope, AuthQueryComment,
            CommentWithReplies, CommentReply, CommentsListEnvelope,
            PostStatusResponse, PostStatusEnvelope,
            ReactRequest
        )
    ),
    tags((name = "chatService", description = "Chat & comments API"))
)]
struct ApiDoc;

// 处理WebSocket连接
async fn handle_connection(
    stream: TcpStream,
    addr: SocketAddr,
    chat_server: Arc<ChatServer>,
) {
    info!("New connection from: {}", addr);
    
    let ws_stream = match accept_async(stream).await {
        Ok(ws) => ws,
        Err(e) => {
            error!("WebSocket connection error: {}", e);
            return;
        }
    };

    let (mut ws_sender, mut ws_receiver) = ws_stream.split();
    let mut user: Option<User> = None;
    let mut message_receiver: Option<broadcast::Receiver<ChatMessage>> = None;

    loop {
        tokio::select! {
            // 处理来自客户端的消息
            msg = ws_receiver.next() => {
                match msg {
                    Some(Ok(Message::Text(text))) => {
                        match serde_json::from_str::<ChatMessage>(&text) {
                            Ok(chat_msg) => {
                                match chat_msg {
                                    ChatMessage::Join { username, room_id } => {
                                        let user_id = Uuid::new_v4();
                                        let (sender, receiver) = broadcast::channel(100);
                                        
                                        let new_user = User {
                                            id: user_id,
                                            username: username.clone(),
                                            room_id: room_id.clone(),
                                            sender,
                                        };
                                        
                                        chat_server.join_room(new_user.clone()).await;
                                        user = Some(new_user);
                                        message_receiver = Some(receiver);
                                        
                                        info!("User {} joined room {}", username, room_id);
                                    }
                                    ChatMessage::Message { username, room_id, content, .. } => {
                                        let timestamp = std::time::SystemTime::now()
                                            .duration_since(std::time::UNIX_EPOCH)
                                            .unwrap()
                                            .as_secs();
                                        
                                        let message = ChatMessage::Message {
                                            username,
                                            room_id: room_id.clone(),
                                            content,
                                            timestamp,
                                        };
                                        
                                        chat_server.broadcast_to_room(&room_id, message).await;
                                    }
                                    _ => {}
                                }
                            }
                            Err(e) => {
                                error!("Failed to parse message: {}", e);
                                let error_msg = ChatMessage::Error {
                                    message: "Invalid message format".to_string(),
                                };
                                if let Ok(error_json) = serde_json::to_string(&error_msg) {
                                    let _ = ws_sender.send(Message::Text(error_json)).await;
                                }
                            }
                        }
                    }
                    Some(Ok(Message::Binary(_))) => {
                        // 忽略二进制消息
                        warn!("Received binary message, ignoring");
                    }
                    Some(Ok(Message::Ping(payload))) => {
                        // 响应ping消息
                        let _ = ws_sender.send(Message::Pong(payload)).await;
                    }
                    Some(Ok(Message::Pong(_))) => {
                        // 忽略pong消息
                    }
                    Some(Ok(Message::Close(_))) => {
                        info!("Client {} disconnected", addr);
                        break;
                    }
                    Some(Ok(Message::Frame(_))) => {
                        // 忽略原始帧
                        warn!("Received raw frame, ignoring");
                    }
                    Some(Err(e)) => {
                        error!("WebSocket error: {}", e);
                        break;
                    }
                    None => break,
                }
            }
            
            // 处理广播消息
            broadcast_msg = async {
                if let Some(ref mut receiver) = message_receiver {
                    receiver.recv().await
                } else {
                    std::future::pending().await
                }
            } => {
                match broadcast_msg {
                    Ok(msg) => {
                        if let Ok(json) = serde_json::to_string(&msg) {
                            if let Err(e) = ws_sender.send(Message::Text(json)).await {
                                error!("Failed to send message: {}", e);
                                break;
                            }
                        }
                    }
                    Err(broadcast::error::RecvError::Closed) => {
                        break;
                    }
                    Err(broadcast::error::RecvError::Lagged(_)) => {
                        warn!("Message receiver lagged");
                    }
                }
            }
        }
    }

    // 清理：用户离开房间
    if let Some(user) = user {
        chat_server.leave_room(user.id, &user.room_id).await;
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // 初始化日志
    tracing_subscriber::fmt::init();

    // Swagger-only 模式：仅启动文档与健康检查，不初始化 DB/Redis 或 WebSocket
    let swagger_only = std::env::var("SWAGGER_ONLY").map(|v| v == "1" || v.eq_ignore_ascii_case("true")).unwrap_or(false);

    // 提前创建 ChatServer，以便通过 HTTP API 与 Python 交互
    let chat_server = Arc::new(ChatServer::new());

    // 初始化数据库与Redis（用于评论模块写入层）
    let comment_service = if !swagger_only {
        let database_url = std::env::var("DATABASE_URL").unwrap_or_else(|_| "postgres://postgres:postgres@localhost:5432/app".to_string());
        let redis_url = std::env::var("REDIS_URL").unwrap_or_else(|_| "redis://127.0.0.1/".to_string());
        let pool = db::init_pg_pool(&database_url).await?;
        let limiter = rate_limit::RateLimiter::new(&redis_url).expect("init redis");
        Some(Arc::new(comments::CommentService::new(pool, limiter)))
    } else {
        None
    };

    // 创建应用状态
    let app_state = AppState {
        chat_server: chat_server.clone(),
        comment_service: comment_service.clone(),
    };

    // 启动 HTTP（Swagger）服务在 8081 端口
    let http_addr: std::net::SocketAddr = "127.0.0.1:8081".parse()?;
    let http_app = Router::new()
        .route("/health", get(health_handler))
        .route("/auth/login", post(login_handler))
        .route("/api/rooms/:room_id/publish", post(publish_handler))
        .route("/api/rooms/:room_id/users", get(list_users_handler))
        .route("/api/rooms/:room_id/search", get(search_users_handler))
        .route("/api/social/action", post(social_action_handler))
        .route("/api/comments", post(create_comment_handler))
        .route("/api/comments/:comment_id", delete(delete_comment_handler))
        .route("/api/posts/:post_id/status", get(check_post_status_handler))
        .route("/api/posts/:post_id/comments", get(get_comments_handler))
        .route("/api/posts/:post_id", delete(delete_post_handler))
        .route("/api/reactions", post(react_handler))
        .merge(SwaggerUi::new("/swagger-ui").url("/api-docs/openapi.json", ApiDoc::openapi()));
    let http_app = http_app.with_state(app_state);
    info!("HTTP Swagger UI listening on: http://{}{}", http_addr, "/swagger-ui/");
    tokio::spawn(async move {
        let http_listener = tokio::net::TcpListener::bind(http_addr).await.unwrap();
        axum::serve(http_listener, http_app).await.unwrap();
    });

    if swagger_only {
        info!("Running in SWAGGER_ONLY mode; skipping DB/Redis & WebSocket server");
        // 阻塞等待 Ctrl+C，以保持进程常驻
        signal::ctrl_c().await?;
        return Ok(());
    }

    // WebSocket 服务继续在 8080 端口
    let addr = "127.0.0.1:8080";
    let listener = TcpListener::bind(&addr).await?;
    info!("Chat server listening on: {}", addr);

    // 使用前面创建的 chat_server

    loop {
        tokio::select! {
            res = listener.accept() => {
                if let Ok((stream, addr)) = res {
                    let chat_server = Arc::clone(&chat_server);
                    tokio::spawn(handle_connection(stream, addr, chat_server));
                }
            }
            _ = signal::ctrl_c() => {
                info!("Shutting down gracefully");
                break;
            }
        }
    }

    Ok(())
}
