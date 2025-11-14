

# 密钥系统集成指南

## 📋 功能概述

实现两种密钥系统：

### 1. 临时操作密钥（128位）
- 根据用户信息 + 时间戳 + 36位随机符号 + User-Agent 生成
- 有效期：3分钟
- 一次性使用
- 仅限创建用户使用
- 双击显示为乱码

### 2. WebSocket 会话密钥（64位）
- 每个聊天会话一个密钥
- 连接期间有效
- 断开连接时自动销毁
- 已存在的会话复用密钥

## 🚀 集成步骤

### 步骤 1: 在 main.rs 中添加模块

```rust
mod secret_key;
use secret_key::{SecretKeyService, TempKeyType};
```

### 步骤 2: 初始化密钥服务

在 `main()` 函数中：

```rust
// 初始化密钥服务
let secret_key_service = Arc::new(SecretKeyService::new(_pool.clone()));
```

### 步骤 3: 添加到 AppState

```rust
#[derive(Clone)]
struct AppState {
    chat_server: Arc<ChatServer>,
    comment_service: Option<Arc<comments::CommentService>>,
    chat_service: Option<Arc<ChatService>>,
    secret_key_service: Arc<SecretKeyService>,  // 新增
}
```

### 步骤 4: 添加 HTTP API 接口

#### 生成临时密钥

```rust
#[derive(Deserialize, ToSchema)]
struct GenerateTempKeyRequest {
    key_type: String,  // "file_download", "file_upload", "api_access", "data_export"
    metadata: Option<String>,
}

#[derive(Serialize, ToSchema)]
struct TempKeyResponse {
    key_value: String,
    expires_at: String,
    obfuscated: String,  // 混淆后的密钥（用于显示）
}

#[utoipa::path(
    post,
    path = "/api/keys/temp/generate",
    request_body = GenerateTempKeyRequest,
    responses(
        (status = 200, description = "密钥生成成功", body = TempKeyEnvelope)
    )
)]
async fn generate_temp_key_handler(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(req): Json<GenerateTempKeyRequest>,
) -> Result<Json<TempKeyEnvelope>, (StatusCode, Json<ApiErrorEnvelope>)> {
    // 从 JWT 或认证信息中获取用户信息
    let user_id = 1; // 示例
    let username = "user1"; // 示例
    
    // 获取 User-Agent
    let user_agent = headers
        .get(axum::http::header::USER_AGENT)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("unknown");
    
    // 解析密钥类型
    let key_type = match req.key_type.as_str() {
        "file_download" => TempKeyType::FileDownload,
        "file_upload" => TempKeyType::FileUpload,
        "api_access" => TempKeyType::ApiAccess,
        "data_export" => TempKeyType::DataExport,
        _ => return Err((
            StatusCode::BAD_REQUEST,
            Json(ApiErrorEnvelope { code: 400, message: "无效的密钥类型".into() })
        )),
    };
    
    // 生成密钥
    let key_value = state.secret_key_service
        .generate_temp_key(user_id, username, user_agent, key_type, req.metadata)
        .await
        .map_err(|e| {
            let status = match e.code() {
                422 => StatusCode::UNPROCESSABLE_ENTITY,
                _ => StatusCode::INTERNAL_SERVER_ERROR,
            };
            (status, Json(ApiErrorEnvelope { code: e.code() as i32, message: e.to_string() }))
        })?;
    
    // 混淆密钥用于显示
    let obfuscated = SecretKeyService::obfuscate_key(&key_value);
    
    let response = TempKeyResponse {
        key_value: key_value.clone(),
        expires_at: (Utc::now() + Duration::minutes(3)).to_rfc3339(),
        obfuscated,
    };
    
    Ok(Json(TempKeyEnvelope {
        code: 0,
        message: "密钥生成成功".into(),
        data: response,
    }))
}
```

#### 验证并使用临时密钥

```rust
#[derive(Deserialize, ToSchema)]
struct ValidateTempKeyRequest {
    key_value: String,
}

#[utoipa::path(
    post,
    path = "/api/keys/temp/validate",
    request_body = ValidateTempKeyRequest,
    responses(
        (status = 200, description = "密钥验证成功"),
        (status = 404, description = "密钥不存在"),
        (status = 410, description = "密钥已过期"),
        (status = 422, description = "密钥已使用或权限不足")
    )
)]
async fn validate_temp_key_handler(
    State(state): State<AppState>,
    Json(req): Json<ValidateTempKeyRequest>,
) -> Result<Json<ValidateKeyEnvelope>, (StatusCode, Json<ApiErrorEnvelope>)> {
    // 从认证信息中获取当前用户ID
    let current_user_id = 1; // 示例
    
    // 验证并使用密钥
    let (user_id, metadata) = state.secret_key_service
        .validate_and_use_temp_key(&req.key_value, current_user_id)
        .await
        .map_err(|e| {
            let status = match e.code() {
                404 => StatusCode::NOT_FOUND,
                410 => StatusCode::GONE,
                422 => StatusCode::UNPROCESSABLE_ENTITY,
                _ => StatusCode::INTERNAL_SERVER_ERROR,
            };
            (status, Json(ApiErrorEnvelope { code: e.code() as i32, message: e.to_string() }))
        })?;
    
    Ok(Json(ValidateKeyEnvelope {
        code: 0,
        message: "密钥验证成功".into(),
        data: ValidateKeyResponse {
            user_id,
            metadata,
        },
    }))
}
```

#### 生成 WebSocket 会话密钥

```rust
#[derive(Deserialize, ToSchema)]
struct GenerateWsKeyRequest {
    conversation_id: i64,
}

#[utoipa::path(
    post,
    path = "/api/keys/ws/generate",
    request_body = GenerateWsKeyRequest,
    responses(
        (status = 200, description = "WebSocket 密钥生成成功")
    )
)]
async fn generate_ws_key_handler(
    State(state): State<AppState>,
    Json(req): Json<GenerateWsKeyRequest>,
) -> Result<Json<WsKeyEnvelope>, (StatusCode, Json<ApiErrorEnvelope>)> {
    // 从认证信息中获取用户ID
    let user_id = 1; // 示例
    
    // 生成 WebSocket 密钥
    let key_value = state.secret_key_service
        .generate_ws_key(user_id, req.conversation_id)
        .await
        .map_err(|e| (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ApiErrorEnvelope { code: 500, message: e.to_string() })
        ))?;
    
    Ok(Json(WsKeyEnvelope {
        code: 0,
        message: "WebSocket 密钥生成成功".into(),
        data: WsKeyResponse { key_value },
    }))
}
```

### 步骤 5: 添加路由

```rust
let http_app = Router::new()
    // 现有路由...
    .route("/api/keys/temp/generate", post(generate_temp_key_handler))
    .route("/api/keys/temp/validate", post(validate_temp_key_handler))
    .route("/api/keys/ws/generate", post(generate_ws_key_handler))
    .with_state(app_state);
```

### 步骤 6: WebSocket 集成

在 WebSocket 连接处理中验证密钥：

```rust
async fn handle_websocket_connection(
    ws: WebSocket,
    state: Arc<AppState>,
    ws_key: String,
) {
    // 验证 WebSocket 密钥
    let (user_id, conversation_id) = match state.secret_key_service
        .validate_ws_key(&ws_key)
        .await
    {
        Ok(info) => info,
        Err(_) => {
            // 密钥无效，关闭连接
            return;
        }
    };
    
    // 处理 WebSocket 消息...
    
    // 连接断开时移除密钥
    let _ = state.secret_key_service.remove_ws_key(&ws_key).await;
}
```

## 📝 使用示例

### Python 客户端示例

```python
import requests
import time

class SecretKeyClient:
    def __init__(self, base_url):
        self.base_url = base_url
    
    # 生成临时密钥
    def generate_temp_key(self, key_type="file_download"):
        response = requests.post(
            f"{self.base_url}/api/keys/temp/generate",
            json={"key_type": key_type}
        )
        result = response.json()
        
        if result["code"] == 0:
            data = result["data"]
            print(f"密钥生成成功:")
            print(f"  原始密钥: {data['key_value']}")
            print(f"  混淆显示: {data['obfuscated']}")
            print(f"  过期时间: {data['expires_at']}")
            return data['key_value']
        else:
            print(f"生成失败: {result['message']}")
            return None
    
    # 使用临时密钥
    def use_temp_key(self, key_value):
        response = requests.post(
            f"{self.base_url}/api/keys/temp/validate",
            json={"key_value": key_value}
        )
        result = response.json()
        
        if result["code"] == 0:
            print("密钥验证成功")
            return True
        else:
            print(f"验证失败: {result['message']}")
            return False
    
    # 生成 WebSocket 密钥
    def generate_ws_key(self, conversation_id):
        response = requests.post(
            f"{self.base_url}/api/keys/ws/generate",
            json={"conversation_id": conversation_id}
        )
        result = response.json()
        
        if result["code"] == 0:
            key = result["data"]["key_value"]
            print(f"WebSocket 密钥: {key}")
            return key
        else:
            print(f"生成失败: {result['message']}")
            return None

# 使用示例
client = SecretKeyClient("http://127.0.0.1:8081")

# 1. 生成临时密钥
key = client.generate_temp_key("file_download")

# 2. 等待一会儿
time.sleep(1)

# 3. 使用密钥
if key:
    client.use_temp_key(key)

# 4. 再次尝试使用（应该失败，因为已使用）
if key:
    client.use_temp_key(key)

# 5. 生成 WebSocket 密钥
ws_key = client.generate_ws_key(conversation_id=1)
```

### 前端示例

```javascript
// 生成临时密钥
async function generateTempKey(keyType = 'file_download') {
    const response = await fetch('/api/keys/temp/generate', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ key_type: keyType })
    });
    
    const result = await response.json();
    
    if (result.code === 0) {
        const { key_value, obfuscated, expires_at } = result.data;
        
        // 显示混淆后的密钥
        document.getElementById('key-display').textContent = obfuscated;
        
        // 实际使用时用原始密钥
        return key_value;
    } else {
        alert(result.message);
        return null;
    }
}

// 使用临时密钥下载文件
async function downloadWithKey(fileId) {
    // 1. 生成密钥
    const key = await generateTempKey('file_download');
    if (!key) return;
    
    // 2. 使用密钥下载
    const response = await fetch(`/api/files/${fileId}/download`, {
        headers: { 'X-Secret-Key': key }
    });
    
    if (response.ok) {
        // 下载文件
        const blob = await response.blob();
        const url = window.URL.createObjectURL(blob);
        const a = document.createElement('a');
        a.href = url;
        a.download = 'file.dat';
        a.click();
    } else {
        const error = await response.json();
        alert(error.message);
    }
}

// WebSocket 连接
async function connectWebSocket(conversationId) {
    // 1. 生成 WebSocket 密钥
    const response = await fetch('/api/keys/ws/generate', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ conversation_id: conversationId })
    });
    
    const result = await response.json();
    if (result.code !== 0) {
        alert(result.message);
        return;
    }
    
    const wsKey = result.data.key_value;
    
    // 2. 使用密钥连接 WebSocket
    const ws = new WebSocket(`ws://127.0.0.1:8080/chat?key=${wsKey}`);
    
    ws.onopen = () => {
        console.log('WebSocket 连接成功');
    };
    
    ws.onmessage = (event) => {
        const message = JSON.parse(event.data);
        console.log('收到消息:', message);
    };
    
    ws.onclose = () => {
        console.log('WebSocket 连接关闭，密钥已销毁');
    };
    
    return ws;
}
```

## 🔒 安全特性

### 临时密钥
1. **生成算法**：SHA-512 哈希，取前128位
2. **组成元素**：用户ID + 用户名 + 时间戳 + 36位随机 + User-Agent
3. **存储**：密钥哈希存储，原始密钥不保存
4. **有效期**：3分钟自动过期
5. **一次性**：使用后立即失效
6. **权限**：仅限创建用户使用
7. **并发**：同一用户同时只能有一个有效密钥

### WebSocket 密钥
1. **生成算法**：SHA-512 哈希，取前64位
2. **组成元素**：ws标识 + 用户ID + 会话ID + 时间戳 + 随机
3. **存储**：仅内存存储，不持久化
4. **有效期**：连接期间有效
5. **复用**：同一会话复用密钥
6. **自动清理**：连接断开时自动销毁

## 🧪 测试

### 测试临时密钥

```python
def test_temp_key_lifecycle():
    client = SecretKeyClient("http://127.0.0.1:8081")
    
    # 1. 生成密钥
    key = client.generate_temp_key()
    assert key is not None
    
    # 2. 第一次使用（成功）
    assert client.use_temp_key(key) == True
    
    # 3. 第二次使用（失败，已使用）
    assert client.use_temp_key(key) == False
    
    # 4. 等待过期
    time.sleep(181)  # 3分钟 + 1秒
    
    # 5. 使用过期密钥（失败）
    key2 = client.generate_temp_key()
    time.sleep(181)
    assert client.use_temp_key(key2) == False

def test_concurrent_key_usage():
    """测试多用户同时使用同一密钥"""
    # 用户A生成密钥
    key = user_a.generate_temp_key()
    
    # 用户B尝试使用（应该失败）
    assert user_b.use_temp_key(key) == False
```

## ✅ 总结

密钥系统已完整实现，包括：
- ✅ 临时操作密钥（128位，3分钟，一次性）
- ✅ WebSocket 会话密钥（64位，连接期间有效）
- ✅ 密钥混淆显示
- ✅ 自动过期清理
- ✅ 并发控制
- ✅ 权限验证

只需按照本指南集成到 main.rs 即可使用！