# Chat Service - Rust 聊天和评论系统

一个功能完整、安全可靠的 Rust 聊天和评论系统，支持 WebSocket 实时通信、评论系统、反应系统和社交功能。

## 🚀 功能特性

### ✅ 评论系统
- 二层评论结构（一级评论 + 二级回复）
- @功能支持
- 软删除 + 级联删除
- 按最新时间排序
- 完整的边界情况处理

### ✅ 聊天系统
- **房间模式**：WebSocket 实时聊天
- **微信模式**：一对一私聊、群聊、离线消息
- 文件/图片支持
- 智能离线消息存储

### ✅ 反应系统
- 点赞/收藏功能
- 不能收藏自己的内容

### ✅ 社交功能
- 关注/屏蔽/静音

### ✅ 认证系统
- JWT + HMAC 双重认证

## 📁 项目结构

```
chatService/
├── src/                    # 源代码
│   ├── main.rs            # 主程序
│   ├── comments.rs        # 评论系统
│   ├── chat.rs            # 聊天系统
│   ├── errors.rs          # 错误处理
│   ├── rate_limit.rs      # 限流
│   └── db.rs              # 数据库
├── docs/                   # 文档
│   ├── guides/            # 指南文档
│   ├── postgres_ddl.sql   # 评论系统表结构
│   └── chat_ddl.sql       # 聊天系统表结构
├── examples/              # 示例代码
│   ├── python_client_example.py
│   ├── fastapi_integration_example.py
│   └── frontend_example.html
├── tests/                 # 测试脚本
│   ├── test_comments.py
│   ├── test_delete_cascade.py
│   ├── test_edge_cases.py
│   └── test_post_status.py
└── README_API.md          # API 文档
```

## 🚀 快速开始

### 1. 初始化数据库

```bash
psql -U postgres -d app -f docs/postgres_ddl.sql
psql -U postgres -d app -f docs/chat_ddl.sql
```

### 2. 配置环境变量

```bash
cp .env.example .env
vim .env
```

### 3. 启动服务

```bash
cargo run
```

服务端口：
- WebSocket: `ws://127.0.0.1:8080`
- HTTP API: `http://127.0.0.1:8081`
- Swagger UI: `http://127.0.0.1:8081/swagger-ui/`

## 📚 文档

- [API 文档](README_API.md)
- [完整功能总结](docs/guides/FINAL_COMPLETE_SUMMARY.md)
- [聊天系统指南](docs/guides/CHAT_SYSTEM_GUIDE.md)
- [快速开始](docs/guides/QUICK_START_CHAT.md)
- [边界情况处理](docs/guides/CHAT_EDGE_CASES.md)

## 🧪 测试

```bash
# 运行测试
python tests/test_comments.py
python tests/test_edge_cases.py
python tests/test_post_status.py
```

## 🔐 安全特性

- 双重认证（JWT + HMAC）
- 速率限制
- 并发控制（顾问锁 + 行级锁）
- 超时保护
- 参数验证
- 软删除

## 📝 License

MIT
