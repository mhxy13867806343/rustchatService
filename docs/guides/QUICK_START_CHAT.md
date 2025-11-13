# 聊天系统快速开始

## 🚀 5分钟快速集成

### 1. 初始化数据库

```bash
# 创建聊天相关的表
psql -U postgres -d app -f docs/chat_ddl.sql
```

### 2. 在 Cargo.toml 中添加依赖（如果需要文件上传）

```toml
[dependencies]
# 现有依赖...

# 文件上传相关
tower-http = { version = "0.5", features = ["fs"] }
```

### 3. 在 src/main.rs 中添加模块

```rust
mod chat;
use chat::{ChatService, MessageType};
```

### 4. 初始化聊天服务

在 `main()` 函数中，数据库初始化后：

```rust
// 初始化聊天服务
let chat_service = Arc::new(ChatService::new(_pool.clone()));
```

### 5. 添加到 AppState

```rust
#[derive(Clone)]
struct AppState {
    chat_server: Arc<ChatServer>,
    comment_service: Option<Arc<comments::CommentService>>,
    chat_service: Option<Arc<ChatService>>,  // 新增
}
```

### 6. 创建聊天服务实例

```rust
let app_state = AppState {
    chat_server: chat_server.clone(),
    comment_service: comment_service.clone(),
    chat_service: Some(chat_service.clone()),  // 新增
};
```

## 📝 最小可用示例

### 创建私聊并发送消息

```rust
use crate::chat::{ChatService, MessageType};

#[tokio::main]
async fn main() {
    // 初始化
    let pool = /* 你的数据库连接池 */;
    let chat_service = ChatService::new(pool);
    
    // 1. 创建私聊会话
    let conversation = chat_service
        .create_private_conversation(1, 2)
        .await
        .unwrap();
    
    println!("创建会话: {:?}", conversation);
    
    // 2. 发送文本消息
    let message = chat_service
        .send_message(
            conversation.id,
            1,  // sender_id
            MessageType::Text,
            "Hello!".to_string(),
            None,
            None,
            None,
        )
        .await
        .unwrap();
    
    println!("发送消息: {:?}", message);
    
    // 3. 用户2上线，获取离线消息
    let offline_messages = chat_service
        .user_online(2, "user2".to_string())
        .await
        .unwrap();
    
    println!("离线消息: {:?}", offline_messages);
}
```

## 🌐 HTTP API 示例

### 添加基本的 HTTP 接口

```rust
// 创建私聊
#[derive(Deserialize)]
struct CreatePrivateChatRequest {
    user1_id: i64,
    user2_id: i64,
}

async fn create_private_chat(
    State(state): State<AppState>,
    Json(req): Json<CreatePrivateChatRequest>,
) -> Result<Json<Conversation>, StatusCode> {
    let chat_service = state.chat_service.as_ref()
        .ok_or(StatusCode::SERVICE_UNAVAILABLE)?;
    
    let conversation = chat_service
        .create_private_conversation(req.user1_id, req.user2_id)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    
    Ok(Json(conversation))
}

// 发送消息
#[derive(Deserialize)]
struct SendMessageRequest {
    conversation_id: i64,
    sender_id: i64,
    content: String,
}

async fn send_message(
    State(state): State<AppState>,
    Json(req): Json<SendMessageRequest>,
) -> Result<Json<Message>, StatusCode> {
    let chat_service = state.chat_service.as_ref()
        .ok_or(StatusCode::SERVICE_UNAVAILABLE)?;
    
    let message = chat_service
        .send_message(
            req.conversation_id,
            req.sender_id,
            MessageType::Text,
            req.content,
            None,
            None,
            None,
        )
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    
    Ok(Json(message))
}

// 添加路由
let http_app = Router::new()
    // 现有路由...
    .route("/api/chat/conversations/private", post(create_private_chat))
    .route("/api/chat/messages", post(send_message))
    .with_state(app_state);
```

## 🧪 测试脚本

创建 `test_chat.py`:

```python
import requests
import time

BASE_URL = "http://127.0.0.1:8081"

# 1. 创建私聊
response = requests.post(f"{BASE_URL}/api/chat/conversations/private", json={
    "user1_id": 1,
    "user2_id": 2
})
conversation = response.json()
print(f"创建会话: {conversation}")

# 2. 发送消息
response = requests.post(f"{BASE_URL}/api/chat/messages", json={
    "conversation_id": conversation["id"],
    "sender_id": 1,
    "content": "Hello from Python!"
})
message = response.json()
print(f"发送消息: {message}")

# 3. 获取会话列表
response = requests.get(f"{BASE_URL}/api/chat/conversations?user_id=2")
conversations = response.json()
print(f"会话列表: {conversations}")
```

## 📱 前端示例

### HTML + JavaScript

```html
<!DOCTYPE html>
<html>
<head>
    <title>聊天测试</title>
</head>
<body>
    <h1>聊天系统测试</h1>
    
    <div>
        <h2>创建私聊</h2>
        <button onclick="createPrivateChat()">创建私聊 (User 1 & 2)</button>
    </div>
    
    <div>
        <h2>发送消息</h2>
        <input type="text" id="messageInput" placeholder="输入消息">
        <button onclick="sendMessage()">发送</button>
    </div>
    
    <div id="messages"></div>
    
    <script>
        let conversationId = null;
        
        async function createPrivateChat() {
            const response = await fetch('/api/chat/conversations/private', {
                method: 'POST',
                headers: { 'Content-Type': 'application/json' },
                body: JSON.stringify({ user1_id: 1, user2_id: 2 })
            });
            const conversation = await response.json();
            conversationId = conversation.id;
            alert('会话创建成功: ' + conversationId);
        }
        
        async function sendMessage() {
            if (!conversationId) {
                alert('请先创建会话');
                return;
            }
            
            const content = document.getElementById('messageInput').value;
            const response = await fetch('/api/chat/messages', {
                method: 'POST',
                headers: { 'Content-Type': 'application/json' },
                body: JSON.stringify({
                    conversation_id: conversationId,
                    sender_id: 1,
                    content: content
                })
            });
            const message = await response.json();
            
            // 显示消息
            const messagesDiv = document.getElementById('messages');
            messagesDiv.innerHTML += `<p>${message.content}</p>`;
            
            document.getElementById('messageInput').value = '';
        }
    </script>
</body>
</html>
```

## ✅ 验证清单

- [ ] 数据库表已创建
- [ ] chat 模块已添加到 main.rs
- [ ] ChatService 已初始化
- [ ] HTTP API 已添加
- [ ] 可以创建私聊会话
- [ ] 可以发送消息
- [ ] 离线消息功能正常

## 🎯 下一步

完成基本集成后，可以继续添加：

1. **群聊功能**
   - 创建群聊 API
   - 邀请成员 API
   - 搜索用户 API

2. **文件上传**
   - 图片上传
   - 文件上传
   - 文件下载

3. **WebSocket 实时推送**
   - 扩展现有 WebSocket
   - 实时消息推送
   - 在线状态同步

4. **前端完整界面**
   - 会话列表
   - 聊天界面
   - 文件预览

## 💡 提示

- 先实现基本的 HTTP API，确保功能正常
- 再添加 WebSocket 实时推送
- 最后完善文件上传和其他高级功能
- 每个功能都要有对应的测试

## 🆘 常见问题

**Q: 如何测试离线消息？**
A: 
1. 用户1发送消息给用户2（用户2离线）
2. 消息会自动保存到 offline_messages 表
3. 用户2上线时调用 `user_online()`
4. 系统会返回所有离线消息并删除记录

**Q: 如何实现群聊？**
A: 
```rust
let conversation = chat_service.create_group_conversation(
    owner_id,
    "群聊名称".to_string(),
    vec![member1_id, member2_id, member3_id]
).await?;
```

**Q: 如何邀请新成员？**
A:
```rust
chat_service.invite_to_group(
    conversation_id,
    inviter_id,
    vec![new_member_id]
).await?;
```

开始构建你的聊天系统吧！🚀
