-- 聊天系统数据库表结构

-- 消息类型枚举
CREATE TYPE message_type AS ENUM ('text', 'image', 'file', 'voice', 'video', 'system');

-- 会话类型枚举
CREATE TYPE conversation_type AS ENUM ('private', 'group');

-- 用户表（简化版，实际应该有更多字段）
CREATE TABLE IF NOT EXISTS users (
    id              BIGSERIAL PRIMARY KEY,
    username        VARCHAR(50) NOT NULL UNIQUE,
    avatar          TEXT,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

-- 会话表
CREATE TABLE IF NOT EXISTS conversations (
    id                  BIGSERIAL PRIMARY KEY,
    conversation_type   conversation_type NOT NULL,
    name                VARCHAR(100),           -- 群聊名称
    avatar              TEXT,                   -- 群聊头像
    owner_id            BIGINT,                 -- 群主ID
    created_at          TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    deleted_at          TIMESTAMPTZ
);

CREATE INDEX idx_conversations_type ON conversations(conversation_type);
CREATE INDEX idx_conversations_owner ON conversations(owner_id);

-- 会话成员表
CREATE TABLE IF NOT EXISTS conversation_members (
    id                  BIGSERIAL PRIMARY KEY,
    conversation_id     BIGINT NOT NULL REFERENCES conversations(id),
    user_id             BIGINT NOT NULL REFERENCES users(id),
    joined_at           TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    left_at             TIMESTAMPTZ
);

CREATE UNIQUE INDEX idx_conversation_members_unique 
    ON conversation_members(conversation_id, user_id) 
    WHERE left_at IS NULL;

CREATE INDEX idx_conversation_members_user ON conversation_members(user_id);
CREATE INDEX idx_conversation_members_conv ON conversation_members(conversation_id);

-- 消息表
CREATE TABLE IF NOT EXISTS messages (
    id                  BIGSERIAL PRIMARY KEY,
    conversation_id     BIGINT NOT NULL REFERENCES conversations(id),
    sender_id           BIGINT NOT NULL REFERENCES users(id),
    message_type        message_type NOT NULL DEFAULT 'text',
    content             TEXT NOT NULL,
    file_url            TEXT,                   -- 文件/图片URL
    file_name           VARCHAR(255),           -- 文件名
    file_size           BIGINT,                 -- 文件大小（字节）
    created_at          TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    deleted_at          TIMESTAMPTZ
);

CREATE INDEX idx_messages_conversation ON messages(conversation_id, created_at DESC);
CREATE INDEX idx_messages_sender ON messages(sender_id);
CREATE INDEX idx_messages_not_deleted ON messages(conversation_id) WHERE deleted_at IS NULL;

-- 离线消息表（用户离线时保存消息）
CREATE TABLE IF NOT EXISTS offline_messages (
    id                  BIGSERIAL PRIMARY KEY,
    user_id             BIGINT NOT NULL REFERENCES users(id),
    message_id          BIGINT NOT NULL REFERENCES messages(id),
    created_at          TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX idx_offline_messages_user ON offline_messages(user_id);
CREATE INDEX idx_offline_messages_message ON offline_messages(message_id);

-- 文件上传记录表
CREATE TABLE IF NOT EXISTS file_uploads (
    id                  BIGSERIAL PRIMARY KEY,
    user_id             BIGINT NOT NULL REFERENCES users(id),
    file_name           VARCHAR(255) NOT NULL,
    file_path           TEXT NOT NULL,
    file_size           BIGINT NOT NULL,
    file_type           VARCHAR(100),
    mime_type           VARCHAR(100),
    created_at          TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX idx_file_uploads_user ON file_uploads(user_id);

-- 插入测试用户
INSERT INTO users (username, avatar) VALUES 
    ('user1', 'https://example.com/avatar1.jpg'),
    ('user2', 'https://example.com/avatar2.jpg'),
    ('user3', 'https://example.com/avatar3.jpg')
ON CONFLICT (username) DO NOTHING;
