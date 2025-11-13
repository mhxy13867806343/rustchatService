# Rust 聊天服务 API 文档

## 概述

这是一个基于 Rust + WebSocket + HTTP 的聊天和评论服务，支持：
- WebSocket 实时聊天
- HTTP REST API（用于 Python FastAPI 等外部服务调用）
- 评论系统（支持二层评论）
- 反应系统（点赞/收藏）
- 社交功能（关注/屏蔽/静音）
- 双重认证机制（JWT + HMAC 签名）

## 快速开始

### 1. 环境配置

复制 `.env.example` 为 `.env` 并修改配置：

```bash
cp .env.example .env
```

关键配置项：
- `DATABASE_URL`: PostgreSQL 数据库连接
- `REDIS_URL`: Redis 连接
- `JWT_SECRET`: JWT 令牌密钥
- `AUTH_SECRET`: HMAC 签名密钥（用于 Python 调用）
- `SWAGGER_ONLY`: 设置为 `true` 时只启动文档服务

### 2. 初始化数据库

```bash
psql -U postgres -d app -f docs/postgres_ddl.sql
```

### 3. 启动服务

```bash
# 开发模式
cargo run

# 生产模式
cargo build --release
./target/release/chatService
```

服务端口：
- WebSocket: `ws://127.0.0.1:8080`
- HTTP API: `http://127.0.0.1:8081`
- Swagger UI: `http://127.0.0.1:8081/swagger-ui/`

### 4. 仅启动文档服务（不连接数据库）

```bash
SWAGGER_ONLY=true cargo run
```

## 认证机制

服务支持两种认证方式：

### 方式 1: JWT Bearer Token（推荐用于内部服务）

1. 先调用 `/auth/login` 获取 JWT Token
2. 在后续请求的 Header 中携带：`Authorization: Bearer <token>`

### 方式 2: HMAC-SHA256 签名（推荐用于 Python FastAPI 调用）

每个请求需要携带以下查询参数：
- `ts`: 时间戳（秒）
- `nonce`: 随机数（UUID）
- `uid_hash`: 用户唯一标识（36 位字母数字）
- `sig`: HMAC-SHA256 签名（十六进制）

签名生成步骤：
1. 按字母顺序排列所有业务参数（不包括认证参数）
2. 拼接成规范字符串：`param1=value1&param2=value2&ts=xxx&nonce=xxx&uid_hash=xxx`
3. 使用 `AUTH_SECRET` 作为密钥，对规范字符串进行 HMAC-SHA256 签名
4. 将签名转换为十六进制小写字符串

Python 示例：
```python
import hmac
import hashlib

canonical = "room_id=room-001&username=user1&content=hello&ts=1234567890&nonce=abc&uid_hash=xyz"
secret = "sso-secret"
sig = hmac.new(secret.encode(), canonical.encode(), hashlib.sha256).hexdigest()
```

## API 接口

### 健康检查

```
GET /health
```

### 登录（获取 JWT）

```
POST /auth/login
Content-Type: application/json

{
  "username": "py-bot",
  "password": "password"
}
```

### 聊天相关

#### 发布消息到房间

```
POST /api/rooms/{room_id}/publish?ts=xxx&nonce=xxx&uid_hash=xxx&sig=xxx
Content-Type: application/json

{
  "username": "user1",
  "content": "Hello World"
}
```

#### 获取房间用户列表

```
GET /api/rooms/{room_id}/users?ts=xxx&nonce=xxx&uid_hash=xxx&sig=xxx
```

#### 搜索房间用户

```
GET /api/rooms/{room_id}/search?q=keyword&ts=xxx&nonce=xxx&uid_hash=xxx&sig=xxx
```

### 社交功能

```
POST /api/social/action?ts=xxx&nonce=xxx&uid_hash=xxx&sig=xxx
Content-Type: application/json

{
  "action": "follow",  // follow | unfollow | block | unblock | mute | unmute
  "target": "target-username"
}
```

### 评论系统

#### 创建评论

```
POST /api/comments?ts=xxx&nonce=xxx&uid_hash=xxx&sig=xxx
Content-Type: application/json

{
  "post_id": 1,
  "author_id": 100,
  "parent_comment_id": null,  // 一级评论为 null，二级评论填父评论 ID
  "content": "这是一条评论",
  "at_user_id": null,  // 可选，@某人
  "idempotency_key": "uuid"  // 幂等键
}
```

**说明**：
- `parent_comment_id` 为 `null` 时创建一级评论
- `parent_comment_id` 有值时创建二级回复（回复某条一级评论）
- `at_user_id` 可以 @某个用户（通常是被回复的作者或帖子作者）
- 最多支持二层评论结构

#### 检查帖子状态（用于前端验证）

```
GET /api/posts/{post_id}/status?ts=xxx&nonce=xxx&uid_hash=xxx&sig=xxx
```

**使用场景**：
- 用户从列表页点击进入详情页时，先检查帖子是否还存在
- 用户长时间未刷新页面，点击时验证帖子状态
- 防止用户操作已删除的帖子

**返回格式**：
```json
{
  "code": 0,
  "message": "ok",
  "data": {
    "exists": true,
    "deleted": false,
    "locked": false,
    "message": "帖子正常"
  }
}
```

**状态说明**：
- `exists: false` - 帖子不存在（返回 code=404）
- `deleted: true` - 帖子已被删除（返回 code=410）
- `locked: true` - 帖子已锁定，无法评论（返回 code=0）
- 全部为 false 且 code=0 - 帖子正常

**前端使用建议**：
```javascript
// 用户点击进入详情页时
async function enterPostDetail(postId) {
  const status = await checkPostStatus(postId);
  
  if (!status.exists) {
    showToast("帖子不存在");
    return;
  }
  
  if (status.deleted) {
    showToast("帖子已被删除");
    return;
  }
  
  if (status.locked) {
    showToast("帖子已锁定，无法评论");
    // 可以继续查看，但禁用评论功能
  }
  
  // 正常进入详情页
  loadPostDetail(postId);
}
```

#### 获取评论列表（嵌套结构）

```
GET /api/posts/{post_id}/comments?ts=xxx&nonce=xxx&uid_hash=xxx&sig=xxx
```

**注意**：此接口会自动检查帖子状态，如果帖子不存在或已删除，会返回相应错误码

**返回格式**：
```json
{
  "code": 0,
  "message": "ok",
  "data": [
    {
      "id": 1,
      "post_id": 1,
      "author_id": 100,
      "content": "这是一级评论",
      "at_user_id": null,
      "created_at": "2024-01-01T00:00:00Z",
      "replies": [
        {
          "id": 2,
          "author_id": 101,
          "content": "这是对一级评论的回复",
          "at_user_id": 100,
          "created_at": "2024-01-01T00:01:00Z"
        },
        {
          "id": 3,
          "author_id": 102,
          "content": "我也来回复一下",
          "at_user_id": 100,
          "created_at": "2024-01-01T00:02:00Z"
        }
      ]
    },
    {
      "id": 4,
      "post_id": 1,
      "author_id": 103,
      "content": "另一条一级评论",
      "at_user_id": null,
      "created_at": "2024-01-01T00:03:00Z",
      "replies": []
    }
  ]
}
```

**数据结构说明**：
- 返回的是一个数组，每个元素是一条一级评论
- 每条一级评论包含 `replies` 数组，存放所有二级回复
- 二级回复中的 `at_user_id` 表示 @了哪个用户
- **按创建时间降序排列（最新的在前面）**
- 一级评论和二级回复都按最新时间排序

#### 删除帖子（软删除，级联删除）

```
DELETE /api/posts/{post_id}?ts=xxx&nonce=xxx&uid_hash=xxx&sig=xxx
```

**级联删除规则**：
- 删除帖子时，会自动软删除：
  - 该帖子下的所有一级评论
  - 该帖子下的所有二级回复
  - 该帖子上的所有反应（点赞/收藏）
  - 所有评论上的反应

**状态码**：
- `200`: 删除成功
- `404`: 帖子不存在
- `410`: 帖子已被删除（重复删除）

**删除后的限制**：
- 已删除的帖子不能再添加评论
- 尝试评论已删除的帖子会返回 `410 Gone`

#### 删除评论（软删除，级联删除）

```
DELETE /api/comments/{comment_id}?ts=xxx&nonce=xxx&uid_hash=xxx&sig=xxx
```

**级联删除规则**：
- 删除一级评论时，会自动软删除：
  - 该一级评论下的所有二级回复
  - 该一级评论上的所有反应
  - 所有二级回复上的反应
- 删除二级回复时，只删除该回复本身及其反应

**状态码**：
- `200`: 删除成功
- `404`: 评论不存在
- `410`: 评论已被删除（重复删除）

**删除后的限制**：
- 已删除的一级评论不能再添加回复
- 尝试回复已删除的评论会返回 `410 Gone`

### 反应系统

```
POST /api/reactions?ts=xxx&nonce=xxx&uid_hash=xxx&sig=xxx
Content-Type: application/json

{
  "resource_type": 1,  // 1=post, 2=comment
  "resource_id": 1,
  "reactor_id": 100,
  "reaction_type": 1,  // 1=like, 2=favorite
  "idempotency_key": "uuid"
}
```

**反应类型**：
- `1`: 点赞（like）- 可以点赞自己的内容
- `2`: 收藏（favorite）- **不能收藏自己发布的内容**

**限制规则**：
- ✅ 可以点赞自己的帖子/评论
- ❌ 不能收藏自己的帖子/评论（返回 422）
- ❌ 不能对已删除的内容添加反应（返回 410）
- ❌ 不能对不存在的内容添加反应（返回 404）

## Python FastAPI 集成

### 安装依赖

```bash
pip install requests
```

### 使用示例

参考 `python_client_example.py` 文件：

```python
from python_client_example import RustChatClient

# 创建客户端
client = RustChatClient(
    base_url="http://127.0.0.1:8081",
    auth_secret="sso-secret"  # 与 .env 中的 AUTH_SECRET 保持一致
)

# 发布消息
client.publish_message("room-001", "python-user", "Hello from Python!")

# 创建一级评论
comment = client.create_comment(
    post_id=1,
    author_id=100,
    content="来自 Python 的一级评论"
)

# 创建二级回复（回复上面的一级评论）
if comment:
    client.create_comment(
        post_id=1,
        author_id=101,
        content="我来回复你",
        parent_comment_id=comment["id"],  # 指定父评论ID
        at_user_id=100  # @一级评论的作者
    )

# 获取评论列表（嵌套结构）
comments = client.get_comments(post_id=1)
for comment in comments:
    print(f"一级评论: {comment['content']}")
    for reply in comment['replies']:
        at_info = f" @{reply['at_user_id']}" if reply.get('at_user_id') else ""
        print(f"  └─ 回复{at_info}: {reply['content']}")
```

### 在 FastAPI 中集成

```python
from fastapi import FastAPI, HTTPException
from python_client_example import RustChatClient

app = FastAPI()
rust_client = RustChatClient(
    base_url="http://127.0.0.1:8081",
    auth_secret="your-auth-secret"
)

@app.post("/send-message")
async def send_message(room_id: str, username: str, content: str):
    success = rust_client.publish_message(room_id, username, content)
    if not success:
        raise HTTPException(status_code=500, detail="Failed to send message")
    return {"status": "ok"}

@app.post("/create-comment")
async def create_comment(post_id: int, author_id: int, content: str):
    comment = rust_client.create_comment(post_id, author_id, content)
    if not comment:
        raise HTTPException(status_code=500, detail="Failed to create comment")
    return comment
```

## WebSocket 客户端

参考 `test_client.html` 文件，使用浏览器连接 WebSocket：

```javascript
const ws = new WebSocket('ws://127.0.0.1:8080');

// 加入房间
ws.send(JSON.stringify({
  type: 'join',
  username: 'user1',
  room_id: 'room-001'
}));

// 发送消息
ws.send(JSON.stringify({
  type: 'message',
  username: 'user1',
  room_id: 'room-001',
  content: 'Hello!'
}));
```

## 错误码

- `0`: 成功
- `400`: 请求参数错误
- `401`: 认证失败
- `404`: 资源不存在
- `408`: 请求超时
  - 事务超时（30秒）
  - 锁获取超时（10秒）
- `410`: 资源已删除（Gone）
  - 帖子已删除，不能评论
  - 评论已删除，不能回复
  - 重复删除同一资源
  - 对已删除的内容添加反应
- `422`: 参数校验失败
  - 超过最大评论层级（只支持二层）
  - 不能收藏自己发布的内容
- `423`: 资源被锁定
  - 帖子已锁定，不能评论
  - 资源正在被操作（并发冲突）
- `429`: 请求过于频繁（触发限流）
  - 速率限制：用户每秒10次，IP每秒20次
  - 连续评论间隔：最少3秒
- `500`: 服务器内部错误
- `503`: 服务不可用

## 限流策略

- 用户维度：每个用户每秒最多 10 次评论请求
- IP 维度：每个 IP 每秒最多 20 次评论请求

## 评论系统详细说明

### 评论结构

评论系统采用二层结构：

```
帖子
├─ 一级评论1 (可以 @帖子作者)
│  ├─ 二级回复1 (可以 @一级评论作者)
│  ├─ 二级回复2 (可以 @一级评论作者或其他人)
│  └─ 二级回复3
├─ 一级评论2
│  └─ 二级回复1
└─ 一级评论3 (无回复)
```

### 使用场景

1. **发表一级评论**：直接评论帖子
   ```python
   client.create_comment(
       post_id=1,
       author_id=100,
       content="我的看法是..."
   )
   ```

2. **回复一级评论**：创建二级回复
   ```python
   client.create_comment(
       post_id=1,
       author_id=101,
       content="我同意你的观点",
       parent_comment_id=1,  # 一级评论的ID
       at_user_id=100  # @一级评论的作者
   )
   ```

3. **@功能**：在回复中提及某人
   - 一级评论可以 @帖子作者
   - 二级回复可以 @一级评论作者或其他相关用户
   - `at_user_id` 字段用于标记被 @的用户

### 测试评论功能

运行测试脚本：
```bash
python test_comments.py
```

这会创建一个完整的评论树示例，包括：
- 多条一级评论
- 每条一级评论下的多个回复
- 带 @功能的回复
- 点赞功能测试

## 注意事项

1. **密钥安全**：生产环境务必修改 `JWT_SECRET` 和 `AUTH_SECRET`
2. **uid_hash 格式**：必须是 36 位字母数字（建议使用 UUID 去掉横线）
3. **幂等性**：评论和反应接口支持幂等，使用 `idempotency_key` 防止重复提交
4. **评论层级**：最多支持二层评论（一级评论 + 二级回复），不支持三层及以上
5. **软删除**：所有删除操作都是软删除，数据不会真正从数据库删除
6. **级联删除**：
   - 删除帖子 → 级联删除所有评论、回复、反应
   - 删除一级评论 → 级联删除其下的所有二级回复和反应
   - 删除二级回复 → 只删除该回复本身
7. **删除后限制**：
   - 已删除的帖子不能再评论（返回 410）
   - 已删除的评论不能再回复（返回 410）
   - 重复删除返回 410
8. **@功能**：`at_user_id` 仅用于标记，实际的通知逻辑需要在业务层实现
9. **评论排序**：评论列表按最新时间降序排列（最新的在前面）
10. **收藏限制**：不能收藏自己发布的帖子或评论（返回 422）
11. **并发控制**：
    - 使用顾问锁防止并发冲突
    - 锁超时时间：10秒
    - 事务超时时间：30秒
12. **连续评论限制**：同一用户在同一帖子下连续评论最少间隔3秒

## 开发调试

### 查看 Swagger 文档

访问 `http://127.0.0.1:8081/swagger-ui/` 查看完整的 API 文档和在线测试。

### 测试签名生成

使用 Python 脚本测试签名：

```python
import hmac
import hashlib
import time
import uuid

def generate_signature(params: dict, secret: str) -> dict:
    ts = int(time.time())
    nonce = str(uuid.uuid4())
    uid_hash = str(uuid.uuid4()).replace("-", "")
    
    sorted_params = sorted(params.items())
    canonical = "&".join([f"{k}={v}" for k, v in sorted_params])
    canonical += f"&ts={ts}&nonce={nonce}&uid_hash={uid_hash}"
    
    sig = hmac.new(secret.encode(), canonical.encode(), hashlib.sha256).hexdigest()
    
    return {
        "ts": ts,
        "nonce": nonce,
        "uid_hash": uid_hash,
        "sig": sig
    }

# 测试
params = {"room_id": "room-001", "username": "user1", "content": "hello"}
auth = generate_signature(params, "sso-secret")
print(auth)
```

## 性能优化建议

1. 使用连接池复用 HTTP 连接
2. 批量操作使用事务
3. 合理设置 Redis 过期时间
4. 监控数据库慢查询
5. 使用 CDN 加速静态资源

## 故障排查

### 认证失败

- 检查 `AUTH_SECRET` 是否一致
- 检查 `uid_hash` 格式是否正确（36 位字母数字）
- 检查签名生成的规范字符串顺序

### 数据库连接失败

- 检查 `DATABASE_URL` 配置
- 确认 PostgreSQL 服务已启动
- 检查数据库表是否已创建

### Redis 连接失败

- 检查 `REDIS_URL` 配置
- 确认 Redis 服务已启动

## 许可证

MIT
