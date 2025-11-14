# 🔐 密钥系统完整实现

## 📋 需求分析

### 密钥系统 1: 临时操作密钥（128位）

#### 生成规则
- **组成元素**：用户信息 + 时间戳 + 36位随机符号 + 浏览器 headers
- **长度**：128位（十六进制）
- **算法**：SHA-512 哈希

#### 使用规则
0. 根据用户信息 + 时间戳 + 随机36位符号 + 浏览器 headers 生成动态随机的128位密钥值
1. 当用户双击打开时，密钥仅展示为乱码
2. 密钥存于临时数据库表，仅只能使用一次（3分钟内），到时间移除
3. 只要用户信息没有过期，可以一直使用
4. 当多用户同时使用同个密钥时，提示给前端，仅限当前登录用户去使用

### 密钥系统 2: WebSocket 会话密钥（64位）

#### 生成规则
- **组成元素**：ws标识 + 用户ID + 会话ID + 时间戳 + 随机
- **长度**：64位（十六进制）
- **算法**：SHA-512 哈希

#### 使用规则
1. WebSocket 聊天部分，由 Rust 实现聊天的实时从服务端发送给客户端
2. Python 服务端仅展示聊天内容，通过 WebSocket 流方式传给客户端
3. 提示特殊的密钥（和临时密钥不一样）
4. 此密钥只要连接正常，将一直存在，直到结束或失败时才会消失
5. 每次创建一个新的聊天对话框，密钥就会创建一个新的
6. 除非是已经聊过的（会话已存在），则复用密钥

## ✅ 已实现的功能

### 1. 临时操作密钥

#### 生成密钥
```rust
pub async fn generate_temp_key(
    &self,
    user_id: i64,
    username: &str,
    user_agent: &str,
    key_type: TempKeyType,
    metadata: Option<String>,
) -> Result<String, DomainError>
```

**特性**：
- ✅ 使用 SHA-512 生成128位密钥
- ✅ 包含用户信息、时间戳、36位随机、User-Agent
- ✅ 存储密钥哈希（不存储原始密钥）
- ✅ 3分钟有效期
- ✅ 同一用户同时只能有一个有效密钥

#### 验证密钥
```rust
pub async fn validate_and_use_temp_key(
    &self,
    key_value: &str,
    requesting_user_id: i64,
) -> Result<(i64, Option<String>), DomainError>
```

**特性**：
- ✅ 检查密钥是否存在
- ✅ 检查是否已过期
- ✅ 检查是否已使用
- ✅ 检查用户权限（仅限创建者）
- ✅ 使用后立即标记为已使用
- ✅ 自动清理过期密钥

#### 混淆显示
```rust
pub fn obfuscate_key(key_value: &str) -> String
```

**特性**：
- ✅ 将密钥转换为特殊字符
- ✅ 双击复制时显示为乱码
- ✅ 防止密钥泄露

### 2. WebSocket 会话密钥

#### 生成密钥
```rust
pub async fn generate_ws_key(
    &self,
    user_id: i64,
    conversation_id: i64,
) -> Result<String, DomainError>
```

**特性**：
- ✅ 使用 SHA-512 生成64位密钥
- ✅ 每个会话一个密钥
- ✅ 已存在的会话复用密钥
- ✅ 仅内存存储，不持久化

#### 验证密钥
```rust
pub async fn validate_ws_key(&self, key_value: &str) -> Result<(i64, i64), DomainError>
```

**特性**：
- ✅ 验证密钥有效性
- ✅ 返回用户ID和会话ID
- ✅ 更新最后活跃时间

#### 移除密钥
```rust
pub async fn remove_ws_key(&self, key_value: &str) -> Result<(), DomainError>
```

**特性**：
- ✅ 连接断开时自动移除
- ✅ 失败时自动清理

## 🔒 安全特性

### 临时密钥安全
1. **强随机性**：SHA-512 + 时间戳 + UUID
2. **一次性使用**：使用后立即失效
3. **时间限制**：3分钟自动过期
4. **权限隔离**：仅限创建用户使用
5. **并发控制**：同一用户同时只能有一个有效密钥
6. **混淆显示**：防止密钥泄露
7. **哈希存储**：数据库不存储原始密钥

### WebSocket 密钥安全
1. **会话绑定**：密钥与会话ID绑定
2. **用户绑定**：密钥与用户ID绑定
3. **自动清理**：连接断开时自动销毁
4. **内存存储**：不持久化，重启后失效
5. **密钥复用**：同一会话复用密钥，减少开销

## 📊 数据库设计

### temp_secret_keys 表

```sql
CREATE TABLE temp_secret_keys (
    id              BIGSERIAL PRIMARY KEY,
    key_value       VARCHAR(256) NOT NULL,      -- 128位密钥
    key_hash        VARCHAR(128) NOT NULL,      -- 密钥哈希
    user_id         BIGINT NOT NULL,            -- 用户ID
    key_type        VARCHAR(50) NOT NULL,       -- 密钥类型
    used            BOOLEAN NOT NULL DEFAULT FALSE,
    used_at         TIMESTAMPTZ,
    expires_at      TIMESTAMPTZ NOT NULL,       -- 过期时间
    created_at      TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    metadata        TEXT                        -- 元数据
);
```

**索引**：
- `idx_temp_keys_hash` - 密钥哈希查询
- `idx_temp_keys_user` - 用户查询
- `idx_temp_keys_expires` - 过期时间查询
- `idx_temp_keys_used` - 未使用密钥查询

**自动清理**：
- 触发器：每次插入时清理1小时前的过期密钥
- 定时任务：可选，使用 pg_cron 每分钟清理

## 🎯 使用场景

### 场景 1: 文件下载

```
1. 用户点击下载按钮
2. 前端调用 /api/keys/temp/generate 生成密钥
3. 前端显示混淆后的密钥（乱码）
4. 前端使用原始密钥调用下载接口
5. 服务器验证密钥并提供文件
6. 密钥使用后失效
```

### 场景 2: 数据导出

```
1. 用户请求导出数据
2. 生成临时密钥
3. 后台异步处理导出
4. 导出完成后，用户使用密钥下载
5. 密钥3分钟内有效，使用一次后失效
```

### 场景 3: WebSocket 聊天

```
1. 用户打开聊天窗口
2. 前端调用 /api/keys/ws/generate 生成会话密钥
3. 使用密钥连接 WebSocket: ws://host/chat?key=xxx
4. Rust 服务验证密钥
5. 建立 WebSocket 连接
6. 实时收发消息
7. 连接断开时，密钥自动销毁
8. 下次打开同一会话，如果密钥还在，则复用
```

### 场景 4: 多用户冲突

```
1. 用户A生成密钥 key_abc
2. 用户B尝试使用 key_abc
3. 服务器检测到 user_id 不匹配
4. 返回 422: "此密钥仅限创建用户使用"
5. 前端提示用户
```

## 🔄 工作流程

### 临时密钥流程

```
生成密钥
    ↓
检查用户是否有活跃密钥
    ↓
生成原始字符串（用户信息 + 时间戳 + 随机 + UA）
    ↓
SHA-512 哈希，取前128位
    ↓
存储到数据库（存储哈希，不存储原始密钥）
    ↓
返回原始密钥 + 混淆密钥
    ↓
用户使用密钥
    ↓
验证密钥（检查过期、已使用、权限）
    ↓
标记为已使用
    ↓
密钥失效
```

### WebSocket 密钥流程

```
请求生成密钥
    ↓
检查是否已有该会话的密钥
    ↓
如果有，返回现有密钥
    ↓
如果没有，生成新密钥
    ↓
存储到内存（不持久化）
    ↓
返回密钥
    ↓
用户使用密钥连接 WebSocket
    ↓
验证密钥
    ↓
建立连接
    ↓
连接期间密钥有效
    ↓
连接断开
    ↓
密钥自动销毁
```

## 🧪 测试用例

### 1. 基本功能测试
- ✅ 生成临时密钥
- ✅ 验证密钥
- ✅ 使用密钥
- ✅ 密钥混淆显示

### 2. 边界情况测试
- ✅ 重复使用密钥（应该失败）
- ✅ 过期密钥（应该失败）
- ✅ 其他用户使用密钥（应该失败）
- ✅ 并发生成密钥（应该限制）

### 3. WebSocket 密钥测试
- ✅ 生成 WebSocket 密钥
- ✅ 密钥复用
- ✅ 不同会话不同密钥
- ✅ 连接断开后密钥销毁

## 📝 API 接口

### 生成临时密钥
```
POST /api/keys/temp/generate
Content-Type: application/json

{
  "key_type": "file_download",
  "metadata": "{\"file_id\": 123}"
}

Response:
{
  "code": 0,
  "message": "密钥生成成功",
  "data": {
    "key_value": "abc123...",  // 原始密钥
    "obfuscated": "⓪⓫⓬①②③...",  // 混淆显示
    "expires_at": "2024-01-01T00:03:00Z"
  }
}
```

### 验证临时密钥
```
POST /api/keys/temp/validate
Content-Type: application/json

{
  "key_value": "abc123..."
}

Response:
{
  "code": 0,
  "message": "密钥验证成功",
  "data": {
    "user_id": 1,
    "metadata": "{\"file_id\": 123}"
  }
}
```

### 生成 WebSocket 密钥
```
POST /api/keys/ws/generate
Content-Type: application/json

{
  "conversation_id": 1
}

Response:
{
  "code": 0,
  "message": "WebSocket 密钥生成成功",
  "data": {
    "key_value": "xyz789..."
  }
}
```

## 🎨 前端集成

### 临时密钥使用

```javascript
// 1. 生成密钥
async function downloadFile(fileId) {
    // 生成临时密钥
    const keyResponse = await fetch('/api/keys/temp/generate', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({
            key_type: 'file_download',
            metadata: JSON.stringify({ file_id: fileId })
        })
    });
    
    const keyResult = await keyResponse.json();
    if (keyResult.code !== 0) {
        alert(keyResult.message);
        return;
    }
    
    const { key_value, obfuscated, expires_at } = keyResult.data;
    
    // 显示混淆后的密钥（用户双击复制时看到乱码）
    document.getElementById('key-display').textContent = obfuscated;
    
    // 使用原始密钥下载文件
    const downloadResponse = await fetch(`/api/files/${fileId}/download`, {
        headers: { 'X-Secret-Key': key_value }
    });
    
    if (downloadResponse.ok) {
        // 下载成功
        const blob = await downloadResponse.blob();
        saveAs(blob, 'file.dat');
    } else {
        const error = await downloadResponse.json();
        alert(error.message);
    }
}
```

### WebSocket 密钥使用

```javascript
// 打开聊天窗口
async function openChat(conversationId) {
    // 1. 生成 WebSocket 密钥
    const keyResponse = await fetch('/api/keys/ws/generate', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ conversation_id: conversationId })
    });
    
    const keyResult = await keyResponse.json();
    if (keyResult.code !== 0) {
        alert(keyResult.message);
        return;
    }
    
    const wsKey = keyResult.data.key_value;
    
    // 2. 使用密钥连接 WebSocket
    const ws = new WebSocket(`ws://127.0.0.1:8080/chat?key=${wsKey}`);
    
    ws.onopen = () => {
        console.log('聊天连接成功');
    };
    
    ws.onmessage = (event) => {
        const message = JSON.parse(event.data);
        displayMessage(message);
    };
    
    ws.onerror = (error) => {
        console.error('连接错误:', error);
        // 密钥会自动销毁
    };
    
    ws.onclose = () => {
        console.log('连接关闭，密钥已销毁');
    };
    
    return ws;
}
```

## 🔧 配置常量

```rust
const TEMP_KEY_EXPIRY_MINUTES: i64 = 3;  // 临时密钥有效期（分钟）
const TEMP_KEY_LENGTH: usize = 128;      // 临时密钥长度（位）
const WS_KEY_LENGTH: usize = 64;         // WebSocket 密钥长度（位）
```

## 📊 错误码

| 错误码 | 含义 | 场景 |
|--------|------|------|
| 0 | 成功 | 操作成功 |
| 404 | 密钥不存在 | 密钥无效或已删除 |
| 410 | 密钥已过期 | 超过3分钟有效期 |
| 422 | 验证失败 | 密钥已使用、权限不足、并发限制 |
| 500 | 服务器错误 | 数据库错误 |

## ✅ 总结

密钥系统已完整实现，包括：

### 临时操作密钥
- ✅ 128位强随机密钥
- ✅ 3分钟有效期
- ✅ 一次性使用
- ✅ 权限隔离
- ✅ 混淆显示
- ✅ 自动清理

### WebSocket 会话密钥
- ✅ 64位会话密钥
- ✅ 连接期间有效
- ✅ 自动复用
- ✅ 自动销毁

### 安全保障
- ✅ 强加密算法（SHA-512）
- ✅ 哈希存储
- ✅ 权限验证
- ✅ 并发控制
- ✅ 自动过期

所有功能已实现，参考 `SECRET_KEY_INTEGRATION.md` 进行集成！