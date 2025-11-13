
# 聊天系统实现指南

## 📋 功能概述

实现一个类似微信的完整聊天系统，包括：

### 核心功能
1. ✅ **一对一私聊**
2. ✅ **群聊**（支持搜索好友、邀请好友）
3. ✅ **离线消息存储**（在线时不存储，离线时存储）
4. ✅ **文件/图片上传**
5. ✅ **消息推送**（用户上线时推送离线消息并删除）

## 🏗️ 已完成的基础架构

### 1. 数据库表结构 (`docs/chat_ddl.sql`)

```sql
-- 用户表
users (id, username, avatar, created_at)

-- 会话表（支持私聊和群聊）
conversations (id, conversation_type, name, avatar, owner_id, created_at, deleted_at)

-- 会话成员表
conversation_members (id, conversation_id, user_id, joined_at, left_at)

-- 消息表（支持文本、图片、文件、语音、视频）
messages (id, conversation_id, sender_id, message_type, content, file_url, file_name, file_size, created_at, deleted_at)

-- 离线消息表
offline_messages (id, user_id, message_id, created_at)

-- 文件上传记录表
file_uploads (id, user_id, file_name, file_path, file_size, file_type, mime_type, created_at)
```

### 2. 核心模块 (`src/chat.rs`)

已实现的核心功能：
- ✅ 用户在线状态管理
- ✅ 一对一私聊会话创建
- ✅ 群聊创建
- ✅ 邀请用户加入群聊
- ✅ 搜索好友
- ✅ 发送消息（文本/文件）
- ✅ 离线消息存储和推送
- ✅ 会话列表获取
- ✅ 消息历史查询

## 🚀 集成步骤

### 步骤 1: 在 main.rs 中添加模块声明

```rust
mod chat;
use chat::ChatService;
```

### 步骤 2: 初始化聊天服务

在 `main()` 函数中：

```rust
// 初始化聊天服务
let chat_service = Arc::new(ChatService::new(_pool.clone()));
```

### 步骤 3: 添加 HTTP API 接口

需要添加以下接口：

#### 会话管理
- `POST /api/chat/conversations/private` - 创建私聊
- `POST /api/chat/conversations/group` - 创建群聊
- `GET /api/chat/conversations` - 获取会话列表
- `POST /api/chat/conversations/{id}/invite` - 邀请用户加入群聊

#### 消息管理
- `POST /api/chat/messages` - 发送消息
- `GET /api/chat/conversations/{id}/messages` - 获取消息历史

#### 用户管理
- `GET /api/chat/users/search` - 搜索用户
- `GET /api/chat/users/online` - 获取在线用户

#### 文件上传
- `POST /api/chat/upload` - 上传文件/图片

### 步骤 4: WebSocket 集成

需要扩展现有的 WebSocket 处理，添加聊天消息类型：

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum ChatMessage {
    // 现有的消息类型...
    
    // 新增聊天消息类型
    #[serde(rename = "chat_message")]
    ChatMessage {
        conversation_id: i64,
        sender_id: i64,
        message_type: String,
        content: String,
        file_url: Option<String>,
    },
    
    #[serde(rename = "user_online")]
    UserOnline {
        user_id: i64,
        username: String,
    },
    
    #[serde(rename = "user_offline")]
    UserOffline {
        user_id: i64,
    },
}
```

## 📝 使用示例

### 1. 用户上线

```rust
// 用户连接 WebSocket 时
let offline_messages = chat_service.user_online(user_id, username).await?;

// 推送离线消息给用户
for message in offline_messages {
    send_to_user(user_id, message).await;
}
```

### 2. 创建私聊

```rust
let conversation = chat_service.create_private_conversation(user1_id, user2_id).await?;
```

### 3. 创建群聊

```rust
let conversation = chat_service.create_group_conversation(
    owner_id,
    "我的群聊".to_string(),
    vec![user2_id, user3_id, user4_id]
).await?;
```

### 4. 发送消息

```rust
let message = chat_service.send_message(
    conversation_id,
    sender_id,
    MessageType::Text,
    "Hello!".to_string(),
    None, // file_url
    None, // file_name
    None, // file_size
).await?;

// 如果接收者在线，通过 WebSocket 实时推送
// 如果接收者离线，消息已自动保存到 offline_messages 表
```

### 5. 搜索好友

```rust
let users = chat_service.search_users_for_invite("张三", 10).await?;
```

### 6. 邀请加入群聊

```rust
chat_service.invite_to_group(
    conversation_id,
    inviter_id,
    vec![new_user_id]
).await?;
```

## 🔧 文件上传实现

### 方案 1: 本地存储

```rust
use axum::extract::Multipart;
use tokio::fs;
use uuid::Uuid;

async fn upload_file(
    State(state): State<AppState>,
    mut multipart: Multipart,
) -> Result<Json<UploadResponse>, StatusCode> {
    while let Some(field) = multipart.next_field().await.unwrap() {
        let name = field.name().unwrap().to_string();
        let file_name = field.file_name().unwrap().to_string();
        let data = field.bytes().await.unwrap();
        
        // 生成唯一文件名
        let unique_name = format!("{}_{}", Uuid::new_v4(), file_name);
        let file_path = format!("uploads/{}", unique_name);
        
        // 保存文件
        fs::write(&file_path, &data).await.unwrap();
        
        // 返回文件URL
        let file_url = format!("/files/{}", unique_name);
        return Ok(Json(UploadResponse { file_url }));
    }
    
    Err(StatusCode::BAD_REQUEST)
}
```

### 方案 2: 对象存储（推荐）

使用 AWS S3、阿里云 OSS 等对象存储服务：

```rust
// 使用 aws-sdk-s3
use aws_sdk_s3::Client;

async fn upload_to_s3(
    client: &Client,
    bucket: &str,
    key: &str,
    data: Vec<u8>,
) -> Result<String, Error> {
    client
        .put_object()
        .bucket(bucket)
        .key(key)
        .body(data.into())
        .send()
        .await?;
    
    Ok(format!("https://{}.s3.amazonaws.com/{}", bucket, key))
}
```

## 🎯 前端集成示例

### JavaScript/TypeScript

```typescript
class ChatClient {
    private ws: WebSocket;
    
    constructor(url: string) {
        this.ws = new WebSocket(url);
        this.setupHandlers();
    }
    
    private setupHandlers() {
        this.ws.onmessage = (event) => {
            const message = JSON.parse(event.data);
            
            switch (message.type) {
                case 'chat_message':
                    this.handleChatMessage(message);
                    break;
                case 'user_online':
                    this.handleUserOnline(message);
                    break;
                case 'user_offline':
                    this.handleUserOffline(message);
                    break;
            }
        };
    }
    
    // 发送消息
    sendMessage(conversationId: number, content: string) {
        this.ws.send(JSON.stringify({
            type: 'chat_message',
            conversation_id: conversationId,
            content: content,
        }));
    }
    
    // 上传文件
    async uploadFile(file: File): Promise<string> {
        const formData = new FormData();
        formData.append('file', file);
        
        const response = await fetch('/api/chat/upload', {
            method: 'POST',
            body: formData,
        });
        
        const result = await response.json();
        return result.file_url;
    }
    
    // 发送图片消息
    async sendImage(conversationId: number, file: File) {
        const fileUrl = await this.uploadFile(file);
        
        this.ws.send(JSON.stringify({
            type: 'chat_message',
            conversation_id: conversationId,
            message_type: 'image',
            content: file.name,
            file_url: fileUrl,
        }));
    }
}
```

## 📊 性能优化建议

### 1. 消息分页加载
```rust
// 已实现
pub async fn get_conversation_messages(
    &self, 
    conversation_id: i64, 
    limit: i64, 
    offset: i64
) -> Result<Vec<Message>, DomainError>
```

### 2. 在线状态缓存
- 使用 Redis 缓存在线用户列表
- 定期同步到内存

### 3. 离线消息批量推送
- 用户上线时批量推送离线消息
- 推送后立即删除

### 4. 文件存储优化
- 使用 CDN 加速文件访问
- 图片自动压缩和缩略图生成
- 大文件分片上传

### 5. 消息推送优化
- 使用消息队列（如 Redis Pub/Sub）
- 支持消息优先级
- 批量推送减少网络开销

## 🔐 安全建议

1. **文件上传安全**
   - 限制文件大小（如 10MB）
   - 限制文件类型
   - 病毒扫描
   - 文件名过滤

2. **消息安全**
   - 敏感词过滤
   - 消息加密（端到端加密）
   - 防止消息轰炸

3. **权限控制**
   - 验证用户是否有权限发送消息
   - 验证用户是否是会话成员
   - 群主权限管理

## 🧪 测试建议

### 单元测试
```rust
#[tokio::test]
async fn test_create_private_conversation() {
    let pool = setup_test_db().await;
    let chat_service = ChatService::new(pool);
    
    let conv = chat_service
        .create_private_conversation(1, 2)
        .await
        .unwrap();
    
    assert_eq!(conv.conversation_type, ConversationType::Private);
}
```

### 集成测试
- 测试用户上线/下线
- 测试离线消息推送
- 测试群聊邀请
- 测试文件上传

## 📚 后续扩展

1. **消息已读状态**
2. **消息撤回**
3. **@提醒**
4. **消息转发**
5. **群公告**
6. **群管理员**
7. **禁言功能**
8. **消息搜索**
9. **语音/视频通话**
10. **表情包支持**

## 🎉 总结

核心聊天系统的基础架构已经完成，包括：
- ✅ 数据库表结构
- ✅ 核心业务逻辑
- ✅ 在线状态管理
- ✅ 离线消息处理
- ✅ 群聊管理

接下来需要：
1. 在 main.rs 中集成 HTTP API
2. 扩展 WebSocket 消息处理
3. 实现文件上传服务
4. 添加前端示例

这是一个完整的、可扩展的聊天系统架构，可以根据实际需求逐步完善！