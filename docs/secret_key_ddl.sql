-- 密钥系统数据库表结构

-- 临时操作密钥表
CREATE TABLE IF NOT EXISTS temp_secret_keys (
    id              BIGSERIAL PRIMARY KEY,
    key_value       VARCHAR(256) NOT NULL,      -- 128位密钥（加密存储）
    key_hash        VARCHAR(128) NOT NULL,      -- 密钥哈希（用于查询）
    user_id         BIGINT NOT NULL,            -- 用户ID
    key_type        VARCHAR(50) NOT NULL,       -- 密钥类型
    used            BOOLEAN NOT NULL DEFAULT FALSE, -- 是否已使用
    used_at         TIMESTAMPTZ,                -- 使用时间
    expires_at      TIMESTAMPTZ NOT NULL,       -- 过期时间
    created_at      TIMESTAMPTZ NOT NULL DEFAULT NOW(), -- 创建时间
    metadata        TEXT                        -- 元数据（JSON）
);

-- 索引
CREATE INDEX idx_temp_keys_hash ON temp_secret_keys(key_hash);
CREATE INDEX idx_temp_keys_user ON temp_secret_keys(user_id);
CREATE INDEX idx_temp_keys_expires ON temp_secret_keys(expires_at);
CREATE INDEX idx_temp_keys_used ON temp_secret_keys(used) WHERE used = false;

-- 自动清理过期密钥的函数
CREATE OR REPLACE FUNCTION cleanup_expired_temp_keys()
RETURNS void AS $$
BEGIN
    DELETE FROM temp_secret_keys WHERE expires_at < NOW();
END;
$$ LANGUAGE plpgsql;

-- 创建定时任务（需要 pg_cron 扩展）
-- 每分钟清理一次过期密钥
-- SELECT cron.schedule('cleanup-temp-keys', '* * * * *', 'SELECT cleanup_expired_temp_keys()');

-- 或者使用触发器在查询时清理
CREATE OR REPLACE FUNCTION trigger_cleanup_expired_keys()
RETURNS TRIGGER AS $$
BEGIN
    DELETE FROM temp_secret_keys WHERE expires_at < NOW() - INTERVAL '1 hour';
    RETURN NEW;
END;
$$ LANGUAGE plpgsql;

CREATE TRIGGER cleanup_on_insert
    AFTER INSERT ON temp_secret_keys
    FOR EACH STATEMENT
    EXECUTE FUNCTION trigger_cleanup_expired_keys();
