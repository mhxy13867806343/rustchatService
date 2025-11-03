use futures_util::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{broadcast, RwLock};
use tokio_tungstenite::{accept_async, tungstenite::Message};
use tracing::{error, info, warn};
use uuid::Uuid;

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
}

impl ChatServer {
    pub fn new() -> Self {
        let (global_sender, _) = broadcast::channel(1000);
        Self {
            rooms: Arc::new(RwLock::new(HashMap::new())),
            global_sender,
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
}

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

    let addr = "127.0.0.1:8080";
    let listener = TcpListener::bind(&addr).await?;
    info!("Chat server listening on: {}", addr);

    let chat_server = Arc::new(ChatServer::new());

    while let Ok((stream, addr)) = listener.accept().await {
        let chat_server = Arc::clone(&chat_server);
        tokio::spawn(handle_connection(stream, addr, chat_server));
    }

    Ok(())
}
