-- DDL: 评论/回复/点赞/收藏/删除 模块
-- 分区策略：按 post_id 哈希分区（简化：主表 + 索引）

CREATE TABLE IF NOT EXISTS posts (
    id              BIGSERIAL PRIMARY KEY,
    author_id       BIGINT NOT NULL,
    title           TEXT,
    content         TEXT,
    locked_at       TIMESTAMPTZ,
    deleted_at      TIMESTAMPTZ,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at      TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

-- 二层评论：一级评论的 parent_comment_id = NULL；二级评论指向一级
CREATE TABLE IF NOT EXISTS comments (
    id                      BIGSERIAL PRIMARY KEY,
    post_id                 BIGINT NOT NULL REFERENCES posts(id),
    author_id               BIGINT NOT NULL,
    parent_comment_id       BIGINT REFERENCES comments(id),
    content                 TEXT NOT NULL,
    at_user_id              BIGINT,
    idempotency_key         TEXT NOT NULL,
    deleted_at              TIMESTAMPTZ,
    created_at              TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at              TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    CHECK (parent_comment_id IS NULL OR at_user_id IS NULL OR at_user_id >= 0)
);

CREATE UNIQUE INDEX IF NOT EXISTS uq_comment_idem
    ON comments(author_id, post_id, idempotency_key);

CREATE INDEX IF NOT EXISTS idx_comments_post_not_deleted
    ON comments(post_id)
    WHERE deleted_at IS NULL;

-- reactions: 点赞/收藏
CREATE TABLE IF NOT EXISTS reactions (
    id              BIGSERIAL PRIMARY KEY,
    resource_type   SMALLINT NOT NULL, -- 1=post, 2=comment
    resource_id     BIGINT NOT NULL,
    reactor_id      BIGINT NOT NULL,
    reaction_type   SMALLINT NOT NULL, -- 1=like, 2=favorite
    idempotency_key TEXT NOT NULL,
    deleted_at      TIMESTAMPTZ,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at      TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE UNIQUE INDEX IF NOT EXISTS uq_reaction_unique
    ON reactions(reactor_id, resource_type, resource_id, reaction_type, idempotency_key);

CREATE INDEX IF NOT EXISTS idx_reactions_not_deleted
    ON reactions(resource_type, resource_id, reaction_type)
    WHERE deleted_at IS NULL;

-- 审计日志
CREATE TABLE IF NOT EXISTS audit_log (
    id              BIGSERIAL PRIMARY KEY,
    actor_id        BIGINT NOT NULL,
    action          TEXT NOT NULL,
    resource_type   TEXT NOT NULL,
    resource_id     BIGINT,
    ip              INET,
    user_agent      TEXT,
    trace_id        TEXT,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

-- 事件通知触发器（简化为NOTIFY，生产中可用逻辑触发器）
-- 由于payload限制8KB，这里仅发送资源类型和ID
-- 示例：NOTIFY events, '{"type":"comment_created","id":123}'